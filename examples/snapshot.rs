//! Renders a frame to a PPM image: cargo run --example snapshot -- india 40 1 out.ppm [zoom cx cy]
use a0::era::Setting;
use a0::render::{render, Camera, Framebuffer};
use a0::sim::Simulation;
use std::io::Write;
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let setting = if a[0] == "india" {
        Setting::India
    } else {
        Setting::NewWorld
    };
    let months: u32 = a[1].parse().unwrap();
    let seed: u64 = a[2].parse().unwrap();
    let mut sim = Simulation::scenario(setting, seed);
    for _ in 0..months {
        sim.step_month();
    }
    let mut fb = Framebuffer::new(1000, 800);
    let mut cam = Camera::fit(sim.width(), sim.height(), &fb);
    if a.len() > 6 {
        cam.zoom = a[4].parse().unwrap();
        cam.x = a[5].parse().unwrap();
        cam.y = a[6].parse().unwrap();
    }
    let unit = sim.unit_near(cam.x, cam.y, 200.0);
    render(&sim, &cam, &mut fb, None, unit);
    let mut f = std::fs::File::create(&a[3]).unwrap();
    write!(f, "P6\n{} {}\n255\n", fb.width, fb.height).unwrap();
    for p in &fb.pixels {
        f.write_all(&[
            (*p & 255) as u8,
            (*p >> 8 & 255) as u8,
            (*p >> 16 & 255) as u8,
        ])
        .unwrap();
    }
    eprintln!("{}", a0::sim::format_date(sim.month()));
}
