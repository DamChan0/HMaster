use cpal::{
    Sample,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use std::sync::{
    Arc,
    atomic::{AtomicF32, Ordering},
};

fn main() -> anyhow::Result<()> {
    let host = cpal::default_host();
    let device = host.default_output_device().expect("출력 디바이스 없음");
}
