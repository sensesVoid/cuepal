mod platform;

use platform::version::{is_supported, os_build};

fn main() {
    env_logger::init();
    let build = os_build();
    if !is_supported(build) {
        eprintln!(
            "CuePal needs Windows 10 build 19041 or later (found build {build}). Not launching."
        );
        std::process::exit(1);
    }
    log::info!("cuepal m0 scaffold ok (os build {build})");
}
