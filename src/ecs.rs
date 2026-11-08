//! A tiny Entity-Component-System, written from scratch.
//!
//! - An [`Entity`] is just an id. It holds no data itself.
//! - A *component* is any plain struct attached to an entity (a position, a city, ...).
//!   All components of one type live together in a [`Storage`].
//! - A *resource* is a global singleton that isn't tied to an entity (the map, the clock).
//! - A *system* is a plain function that runs over the world once per tick.
//!
//! Systems get `&World` and borrow the storages they need at runtime through `RefCell`,
//! so one system can read positions while writing cities. They can't spawn or despawn
//! entities while iterating, so they queue those changes in [`Commands`], which the
//! [`Schedule`] applies once the system returns.

use std::any::{type_name, Any, TypeId};
use std::cell::{Ref, RefCell, RefMut};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Entity {
    index: u32,
    generation: u32,
}

impl Entity {
    pub fn index(self) -> usize {
        self.index as usize
    }
}

/// Hands out entity ids and recycles freed slots. Each slot has a generation that is
/// bumped on despawn, so an old `Entity` kept somewhere can be told apart from the new
/// entity that reused its slot.
#[derive(Default)]
struct Entities {
    generations: Vec<u32>,
    alive: Vec<bool>,
    free: Vec<u32>,
}

impl Entities {
    fn spawn(&mut self) -> Entity {
        if let Some(index) = self.free.pop() {
            self.alive[index as usize] = true;
            Entity {
                index,
                generation: self.generations[index as usize],
            }
        } else {
            let index = self.generations.len() as u32;
            self.generations.push(0);
            self.alive.push(true);
            Entity {
                index,
                generation: 0,
            }
        }
    }

    fn is_alive(&self, e: Entity) -> bool {
        self.alive.get(e.index()).copied().unwrap_or(false)
            && self.generations[e.index()] == e.generation
    }

    fn despawn(&mut self, e: Entity) -> bool {
        if !self.is_alive(e) {
            return false;
        }
        self.alive[e.index()] = false;
        self.generations[e.index()] += 1;
        self.free.push(e.index);
        true
    }

    fn count(&self) -> usize {
        self.alive.iter().filter(|alive| **alive).count()
    }
}

/// All components of one type, stored as a "sparse set": the components are packed
/// tightly in `dense` (fast to iterate), `owners[i]` says which entity `dense[i]`
/// belongs to, and `sparse[entity index]` says where that entity's component sits.
pub struct Storage<T> {
    dense: Vec<T>,
    owners: Vec<Entity>,
    sparse: Vec<Option<u32>>,
}

impl<T> Default for Storage<T> {
    fn default() -> Self {
        Self {
            dense: Vec::new(),
            owners: Vec::new(),
            sparse: Vec::new(),
        }
    }
}

impl<T> Storage<T> {
    fn slot(&self, e: Entity) -> Option<usize> {
        let slot = (*self.sparse.get(e.index())?)? as usize;
        (self.owners[slot] == e).then_some(slot)
    }

    pub fn insert(&mut self, e: Entity, value: T) {
        if let Some(slot) = self.slot(e) {
            self.dense[slot] = value;
            return;
        }
        if self.sparse.len() <= e.index() {
            self.sparse.resize(e.index() + 1, None);
        }
        self.sparse[e.index()] = Some(self.dense.len() as u32);
        self.dense.push(value);
        self.owners.push(e);
    }

    /// Removes in O(1) by moving the last component into the freed slot.
    pub fn remove(&mut self, e: Entity) -> Option<T> {
        let slot = self.slot(e)?;
        self.sparse[e.index()] = None;
        let value = self.dense.swap_remove(slot);
        self.owners.swap_remove(slot);
        if let Some(&moved) = self.owners.get(slot) {
            self.sparse[moved.index()] = Some(slot as u32);
        }
        Some(value)
    }

    pub fn get(&self, e: Entity) -> Option<&T> {
        self.slot(e).map(|slot| &self.dense[slot])
    }

    pub fn get_mut(&mut self, e: Entity) -> Option<&mut T> {
        let slot = self.slot(e)?;
        Some(&mut self.dense[slot])
    }

    pub fn contains(&self, e: Entity) -> bool {
        self.slot(e).is_some()
    }

    pub fn len(&self) -> usize {
        self.dense.len()
    }

    pub fn is_empty(&self) -> bool {
        self.dense.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (Entity, &T)> + '_ {
        self.owners.iter().copied().zip(self.dense.iter())
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (Entity, &mut T)> + '_ {
        self.owners.iter().copied().zip(self.dense.iter_mut())
    }
}

/// Lets the world treat storages of different component types uniformly, e.g. to
/// remove a despawned entity from all of them.
trait AnyStorage {
    fn remove_entity(&self, e: Entity);
    fn as_any(&self) -> &dyn Any;
}

impl<T: 'static> AnyStorage for RefCell<Storage<T>> {
    fn remove_entity(&self, e: Entity) {
        self.borrow_mut().remove(e);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[derive(Default)]
pub struct World {
    entities: Entities,
    components: HashMap<TypeId, Box<dyn AnyStorage>>,
    resources: HashMap<TypeId, Box<dyn Any>>,
}

impl World {
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates the storage for `T`. Inserting a component does this automatically;
    /// call it up front for types a system may read before any entity has one.
    pub fn register<T: 'static>(&mut self) {
        self.components
            .entry(TypeId::of::<T>())
            .or_insert_with(|| Box::new(RefCell::new(Storage::<T>::default())));
    }

    /// Spawns an entity with a bundle of components, e.g. `world.spawn((pos, city))`.
    pub fn spawn(&mut self, bundle: impl Bundle) -> Entity {
        let e = self.entities.spawn();
        bundle.insert_into(self, e);
        e
    }

    pub fn insert<T: 'static>(&mut self, e: Entity, component: T) {
        if !self.entities.is_alive(e) {
            return;
        }
        self.register::<T>();
        self.storage::<T>().borrow_mut().insert(e, component);
    }

    pub fn remove<T: 'static>(&mut self, e: Entity) -> Option<T> {
        self.try_storage::<T>()?.borrow_mut().remove(e)
    }

    pub fn despawn(&mut self, e: Entity) {
        if self.entities.despawn(e) {
            for storage in self.components.values() {
                storage.remove_entity(e);
            }
        }
    }

    pub fn is_alive(&self, e: Entity) -> bool {
        self.entities.is_alive(e)
    }

    pub fn entity_count(&self) -> usize {
        self.entities.count()
    }

    fn try_storage<T: 'static>(&self) -> Option<&RefCell<Storage<T>>> {
        let storage = self.components.get(&TypeId::of::<T>())?;
        Some(
            storage
                .as_any()
                .downcast_ref()
                .expect("storage type mismatch"),
        )
    }

    fn storage<T: 'static>(&self) -> &RefCell<Storage<T>> {
        self.try_storage()
            .unwrap_or_else(|| panic!("component `{}` was never registered", type_name::<T>()))
    }

    pub fn read<T: 'static>(&self) -> Ref<'_, Storage<T>> {
        self.storage::<T>()
            .try_borrow()
            .unwrap_or_else(|_| panic!("`{}` is already borrowed for writing", type_name::<T>()))
    }

    pub fn write<T: 'static>(&self) -> RefMut<'_, Storage<T>> {
        self.storage::<T>()
            .try_borrow_mut()
            .unwrap_or_else(|_| panic!("`{}` is already borrowed", type_name::<T>()))
    }

    pub fn insert_resource<T: 'static>(&mut self, resource: T) {
        self.resources
            .insert(TypeId::of::<T>(), Box::new(RefCell::new(resource)));
    }

    fn resource_cell<T: 'static>(&self) -> &RefCell<T> {
        self.resources
            .get(&TypeId::of::<T>())
            .and_then(|r| r.downcast_ref())
            .unwrap_or_else(|| panic!("resource `{}` was never inserted", type_name::<T>()))
    }

    pub fn resource<T: 'static>(&self) -> Ref<'_, T> {
        self.resource_cell::<T>().try_borrow().unwrap_or_else(|_| {
            panic!(
                "resource `{}` is already borrowed for writing",
                type_name::<T>()
            )
        })
    }

    pub fn resource_mut<T: 'static>(&self) -> RefMut<'_, T> {
        self.resource_cell::<T>()
            .try_borrow_mut()
            .unwrap_or_else(|_| panic!("resource `{}` is already borrowed", type_name::<T>()))
    }
}

/// A group of components spawned together. Implemented for tuples of up to six
/// components by the macro below.
pub trait Bundle: 'static {
    fn insert_into(self, world: &mut World, e: Entity);
}

impl Bundle for () {
    fn insert_into(self, _: &mut World, _: Entity) {}
}

macro_rules! impl_bundle {
    ($($name:ident),+) => {
        impl<$($name: 'static),+> Bundle for ($($name,)+) {
            #[allow(non_snake_case)]
            fn insert_into(self, world: &mut World, e: Entity) {
                let ($($name,)+) = self;
                $(world.insert(e, $name);)+
            }
        }
    };
}

impl_bundle!(A);
impl_bundle!(A, B);
impl_bundle!(A, B, C);
impl_bundle!(A, B, C, D);
impl_bundle!(A, B, C, D, E);
impl_bundle!(A, B, C, D, E, F);

type Command = Box<dyn FnOnce(&mut World)>;

/// Structural changes queued by a system, applied after it finishes.
#[derive(Default)]
pub struct Commands {
    queue: Vec<Command>,
}

impl Commands {
    pub fn add(&mut self, command: impl FnOnce(&mut World) + 'static) {
        self.queue.push(Box::new(command));
    }

    pub fn spawn(&mut self, bundle: impl Bundle) {
        self.add(move |world| {
            world.spawn(bundle);
        });
    }

    pub fn despawn(&mut self, e: Entity) {
        self.add(move |world| world.despawn(e));
    }

    pub fn apply(self, world: &mut World) {
        for command in self.queue {
            command(world);
        }
    }
}

pub type System = fn(&World, &mut Commands);

/// An ordered list of systems.
#[derive(Default)]
pub struct Schedule {
    systems: Vec<(&'static str, System)>,
}

impl Schedule {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, name: &'static str, system: System) -> Self {
        self.systems.push((name, system));
        self
    }

    pub fn run(&self, world: &mut World) {
        for &(_, system) in &self.systems {
            let mut commands = Commands::default();
            system(world, &mut commands);
            commands.apply(world);
        }
    }

    pub fn names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.systems.iter().map(|(name, _)| *name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq)]
    struct Pos(i32);
    #[derive(Debug, PartialEq)]
    struct Vel(i32);

    #[test]
    fn spawn_and_read_components() {
        let mut world = World::new();
        let a = world.spawn((Pos(1), Vel(2)));
        let b = world.spawn((Pos(10),));
        assert_eq!(world.read::<Pos>().get(a), Some(&Pos(1)));
        assert_eq!(world.read::<Vel>().get(a), Some(&Vel(2)));
        assert_eq!(world.read::<Vel>().get(b), None);
        assert_eq!(world.read::<Pos>().len(), 2);
    }

    #[test]
    fn despawn_removes_all_components_and_old_ids_go_stale() {
        let mut world = World::new();
        let a = world.spawn((Pos(1), Vel(1)));
        world.despawn(a);
        assert!(!world.is_alive(a));
        assert!(world.read::<Pos>().is_empty());
        assert!(world.read::<Vel>().is_empty());

        // The slot is reused, but the old id must not see the new entity's data.
        let b = world.spawn((Pos(2),));
        assert_eq!(a.index(), b.index());
        assert_eq!(world.read::<Pos>().get(a), None);
        assert_eq!(world.read::<Pos>().get(b), Some(&Pos(2)));
        world.insert(a, Pos(99)); // ignored: `a` is dead
        assert_eq!(world.read::<Pos>().get(b), Some(&Pos(2)));
    }

    #[test]
    fn removing_from_the_middle_keeps_lookups_correct() {
        let mut world = World::new();
        let ids: Vec<_> = (0..5).map(|i| world.spawn((Pos(i),))).collect();
        world.despawn(ids[1]);
        for (i, &e) in ids.iter().enumerate().filter(|(i, _)| *i != 1) {
            assert_eq!(world.read::<Pos>().get(e), Some(&Pos(i as i32)));
        }
    }

    #[test]
    fn systems_can_read_one_storage_while_writing_another() {
        fn movement(world: &World, _: &mut Commands) {
            let vel = world.read::<Vel>();
            let mut pos = world.write::<Pos>();
            for (e, p) in pos.iter_mut() {
                if let Some(v) = vel.get(e) {
                    p.0 += v.0;
                }
            }
        }
        let mut world = World::new();
        let a = world.spawn((Pos(0), Vel(3)));
        let still = world.spawn((Pos(7),));
        let schedule = Schedule::new().with("movement", movement);
        schedule.run(&mut world);
        schedule.run(&mut world);
        assert_eq!(world.read::<Pos>().get(a), Some(&Pos(6)));
        assert_eq!(world.read::<Pos>().get(still), Some(&Pos(7)));
    }

    #[test]
    fn commands_apply_after_the_system() {
        fn spawner(world: &World, commands: &mut Commands) {
            assert_eq!(world.read::<Pos>().len(), 1);
            commands.spawn((Pos(5),));
            assert_eq!(world.read::<Pos>().len(), 1, "spawn must be deferred");
        }
        let mut world = World::new();
        world.spawn((Pos(0),));
        Schedule::new().with("spawner", spawner).run(&mut world);
        assert_eq!(world.read::<Pos>().len(), 2);
    }

    #[test]
    fn resources() {
        let mut world = World::new();
        world.insert_resource(41u32);
        *world.resource_mut::<u32>() += 1;
        assert_eq!(*world.resource::<u32>(), 42);
    }

    #[test]
    #[should_panic(expected = "already borrowed")]
    fn conflicting_borrows_panic_with_the_type_name() {
        let mut world = World::new();
        world.register::<Pos>();
        let _reading = world.read::<Pos>();
        let _writing = world.write::<Pos>();
    }
}
