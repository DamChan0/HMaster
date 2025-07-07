mod FilePlay;
mod equalization;
mod loopback;
mod loopback_eq;

use anyhow::{Result, anyhow};
use clap::Parser;
use core::error;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait}; // DeviceTrait 추가
use equalization::{BiquadEq, EqError, select_loopback_input_device, select_output_device};
use loopback::Loopback;
use std::sync::Arc;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Cli {
    /// 볼륨 레벨 (0.0 ~ 1.0, 기본값: 1.0)
    #[arg(short, long, default_value_t = 1.0)]
    volume: f32,
}

fn main() -> Result<()> {
    let input_device = equalization::select_loopback_input_device()?; // loopback 없으면 default mic

    let output_device = match select_output_device() {
        Ok(device) => device,
        Err(EqError::NoOutputDevice) => {
            return Err(anyhow!("출력 장치를 찾을 수 없습니다."));
        }
        Err(e) => {
            return Err(anyhow!("출력 장치 선택 오류: {:?}", e));
        }
    };
    for config in input_device.supported_input_configs()? {
        println!("지원 입력: {:?}", config);
    }
    for config in output_device.supported_output_configs()? {
        println!("지원 출력: {:?}", config);
    }
    println!("선택된 입력 장치: {}", input_device.name()?);
    println!("선택된 출력 장치: {}", output_device.name()?);

    // 2) 볼륨 및 EQ 설정
    let volume = 1.0;

    // 3) EQ 루프백 스트림 생성 및 실행
    let lb = loopback_eq::LoopbackWithEq::new(input_device, output_device, volume)?;
    lb.play()?;

    println!("EQ 모드 실행 중... 종료하려면 Ctrl+C");
    std::thread::park(); // 대기

    Ok(())
}
