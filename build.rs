use std::str;
use std::process::Command;

fn main() {
    let output: Vec<u8> = Command::new("rustc")
        .arg("--version")
        .output()
        .unwrap()
        .stdout;
    let output: Vec<&str> = output
        .split(|k| *k == b' ')
        .map(|k| str::from_utf8(k).unwrap())
        .collect();
    assert_eq!(output[0], "rustc");
    let version: Vec<&str> = output[1].split(".").collect();
    assert_eq!(version[0], "1");
    let minor: u32 = version[1].parse().unwrap();
    if minor < 30 {
        println!("cargo:rustc-cfg=rust_legacy_error");
    }
}