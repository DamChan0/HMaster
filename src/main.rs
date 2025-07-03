mod FilePlay;
mod loopback;

use anyhow::{Result, anyhow};
use clap::Parser;
use cpal::traits::HostTrait;
use loopback::Loopback;
use rodio::DeviceTrait;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Cli {
    /// 볼륨 레벨 (0.0 ~ 1.0, 기본값: 1.0)
    #[arg(short, long, default_value_t = 1.0)]
    volume: f32,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let volume = cli.volume.clamp(0.0, 1.0);
    println!("오디오 프로세싱 시작. 볼륨: {}%", volume * 100.0);

    let host = cpal::default_host();
    let input_device = host
        .default_input_device()
        .ok_or_else(|| anyhow!("입력 디바이스가 없습니다"))?;
    let output_device = host
        .default_output_device()
        .ok_or_else(|| anyhow!("출력 디바이스가 없습니다"))?;

    println!("입력 장치:  {}", input_device.name()?);
    println!("출력 장치:  {}", output_device.name()?);

    // 파일 재생 (기존 FilePlay 모듈)
    // FilePlay::play_file("audio.wav")?;

    // 실시간 루프백
    let lb = Loopback::new(input_device, output_device, volume)?;
    lb.play()?;

    println!("실시간 루프백 재생 중... Ctrl+C로 종료");
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}
