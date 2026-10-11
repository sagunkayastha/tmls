//! `s2-wgpu` binary. The window, input and CLI arrive with the next task; until then this only
//! reports which GPU adapters wgpu finds, which is enough to check a build on a new machine.

fn main() {
    let adapters = s2_wgpu::gfx::render::adapter_report();
    if adapters.is_empty() {
        println!("s2-wgpu: no wgpu adapter found");
    }
    for adapter in adapters {
        println!("s2-wgpu: adapter {adapter}");
    }
}
