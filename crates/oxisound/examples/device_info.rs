fn main() {
    match oxisound::enumerate_devices() {
        Ok(devices) => {
            if devices.is_empty() {
                println!("No audio devices found.");
            } else {
                println!("{}", oxisound::format_devices(&devices));
            }
        }
        Err(e) => println!("Could not enumerate devices: {e}"),
    }
}
