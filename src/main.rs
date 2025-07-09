// mod FilePlay;
// mod equalization;
// mod loopback;
// mod loopback_eq;

// use anyhow::{Result, anyhow};
// use clap::Parser;
// use core::error;
// use cpal::traits::{DeviceTrait, HostTrait, StreamTrait}; // DeviceTrait 추가
// use equalization::{BiquadEq, EqError, select_loopback_input_device, select_output_device};
// use loopback::Loopback;
// use std::sync::Arc;

// #[derive(Parser, Debug)]
// #[command(version, about, long_about = None)]
// struct Cli {
//     /// 볼륨 레벨 (0.0 ~ 1.0, 기본값: 1.0)
//     #[arg(short, long, default_value_t = 1.0)]
//     volume: f32,
// }

// fn main() -> Result<()> {
//     let input_device = equalization::select_loopback_input_device()?; // loopback 없으면 default mic

//     let output_device = match select_output_device() {
//         Ok(device) => device,
//         Err(EqError::NoOutputDevice) => {
//             return Err(anyhow!("출력 장치를 찾을 수 없습니다."));
//         }
//         Err(e) => {
//             return Err(anyhow!("출력 장치 선택 오류: {:?}", e));
//         }
//     };
//     for config in input_device.supported_input_configs()? {
//         println!("지원 입력: {:?}", config);
//     }
//     for config in output_device.supported_output_configs()? {
//         println!("지원 출력: {:?}", config);
//     }
//     println!("선택된 입력 장치: {}", input_device.name()?);
//     println!("선택된 출력 장치: {}", output_device.name()?);

//     // 2) 볼륨 및 EQ 설정
//     let volume = 1.0;

//     // 3) EQ 루프백 스트림 생성 및 실행
//     let lb = loopback_eq::LoopbackWithEq::new(input_device, output_device, volume)?;
//     lb.play()?;

//     println!("EQ 모드 실행 중... 종료하려면 Ctrl+C");
//     std::thread::park(); // 대기

//     Ok(())
// }
// main.rs

use biquad::{Biquad, Coefficients, DirectForm1, Errors, Q_BUTTERWORTH_F32, ToHertz, Type};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::HeapRb;
use std::io::{Write, stdin};
use std::sync::{Arc, Mutex}; // Arc와 Mutex를 가져옵니다.
use std::thread;

// --- EQ 설정 영역을 'let mut' 변수로 변경하여 런타임에 수정 가능하게 합니다.
struct EqSettings {
    low_shelf_freq: f32,
    low_shelf_gain: f32,
    peaking_eq_freq: f32,
    peaking_eq_gain: f32,
    peaking_eq_q: f32,
    high_shelf_freq: f32,
    high_shelf_gain: f32,
}

fn biquad_err_to_dyn_err(e: Errors) -> Box<dyn std::error::Error> {
    format!("{:?}", e).into()
}

/// 설정값을 기반으로 필터 목록을 다시 생성하는 함수
fn update_filters(
    filters: &Arc<Mutex<Vec<Vec<DirectForm1<f32>>>>>,
    settings: &EqSettings,
    sample_rate: u32,
    channels: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut locked_filters = filters.lock().unwrap(); // Mutex를 잠그고 접근
    locked_filters.clear(); // 기존 필터를 모두 제거

    for _ in 0..channels {
        let mut channel_bands: Vec<DirectForm1<f32>> = Vec::new();

        if settings.low_shelf_gain.abs() > 0.01 {
            let coeffs_low = Coefficients::<f32>::from_params(
                Type::LowShelf(settings.low_shelf_gain),
                (sample_rate as f32).hz(),
                settings.low_shelf_freq.hz(),
                Q_BUTTERWORTH_F32,
            )
            .map_err(biquad_err_to_dyn_err)?;
            channel_bands.push(DirectForm1::<f32>::new(coeffs_low));
        }
        if settings.peaking_eq_gain.abs() > 0.01 {
            let coeffs_peak = Coefficients::<f32>::from_params(
                Type::PeakingEQ(settings.peaking_eq_gain),
                (sample_rate as f32).hz(),
                settings.peaking_eq_freq.hz(),
                settings.peaking_eq_q,
            )
            .map_err(biquad_err_to_dyn_err)?;
            channel_bands.push(DirectForm1::<f32>::new(coeffs_peak));
        }
        if settings.high_shelf_gain.abs() > 0.01 {
            let coeffs_high = Coefficients::<f32>::from_params(
                Type::HighShelf(settings.high_shelf_gain),
                (sample_rate as f32).hz(),
                settings.high_shelf_freq.hz(),
                Q_BUTTERWORTH_F32,
            )
            .map_err(biquad_err_to_dyn_err)?;
            channel_bands.push(DirectForm1::<f32>::new(coeffs_high));
        }
        locked_filters.push(channel_bands);
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // --- (장치 선택 부분은 이전과 동일) ---
    let host = cpal::default_host();
    let input_device = host
        .input_devices()?
        .find(|d| d.name().unwrap_or_default().contains("CABLE Output"))
        .expect("VB-CABLE 입력 장치를 찾을 수 없습니다.");
    println!("입력 장치 (캡처): {}", input_device.name()?);
    let available_outputs: Vec<_> = host
        .output_devices()?
        .filter(|d| !d.name().unwrap_or_default().contains("CABLE"))
        .collect();
    if available_outputs.is_empty() {
        panic!("사용 가능한 출력 장치가 없습니다!");
    }
    println!("\n▶ 실제 소리를 출력할 스피커를 선택하세요:");
    for (i, device) in available_outputs.iter().enumerate() {
        println!("  [{}] {}", i + 1, device.name()?);
    }
    let output_device = loop {
        print!("번호를 입력하세요: ");
        std::io::stdout().flush()?;
        let mut choice_str = String::new();
        stdin().read_line(&mut choice_str)?;
        match choice_str.trim().parse::<usize>() {
            Ok(choice) if choice > 0 && choice <= available_outputs.len() => {
                break available_outputs[choice - 1].clone();
            }
            _ => println!("잘못된 번호입니다. 다시 입력해주세요."),
        }
    };
    println!("\n선택된 출력 장치 (재생): {}", output_device.name()?);
    let config: cpal::StreamConfig = output_device.default_output_config()?.into();
    let sample_rate = config.sample_rate.0;
    let channels = config.channels as usize;

    // --- EQ 설정과 필터를 Arc<Mutex<T>>로 감싸서 생성 ---
    let mut settings = EqSettings {
        low_shelf_gain: 6.0,
        low_shelf_freq: 150.0,
        peaking_eq_gain: -3.0,
        peaking_eq_freq: 1_000.0,
        peaking_eq_q: 1.0,
        high_shelf_gain: 4.0,
        high_shelf_freq: 4_000.0,
    };
    let filters = Arc::new(Mutex::new(Vec::new()));

    // 초기 필터 생성
    update_filters(&filters, &settings, sample_rate, channels)?;

    let latency_ms = 50.0;
    let latency_frames = (latency_ms / 1000.0) * sample_rate as f32;
    let ring_buffer_size = (latency_frames as usize) * channels;
    // --- 스트림 생성 시, Arc를 복제(clone)하여 넘겨줍니다 ---
    let (mut producer, mut consumer) = HeapRb::<f32>::new(ring_buffer_size).split();
    let input_stream = input_device.build_input_stream(
        &config,
        move |data: &[f32], _: &cpal::InputCallbackInfo| {
            producer.push_slice(data);
        },
        |err| eprintln!("입력 스트림 에러: {:?}", err),
        None,
    )?;

    let filters_clone = Arc::clone(&filters); // 오디오 스레드로 보낼 Arc 복제본
    let output_stream = output_device.build_output_stream(
        &config,
        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            let read_count = consumer.pop_slice(data);
            data[read_count..].iter_mut().for_each(|s| *s = 0.0);

            let mut locked_filters = filters_clone.lock().unwrap(); // 오디오 처리 중 필터 잠금

            for (i, sample) in data.iter_mut().enumerate() {
                let channel_index = i % channels;
                let mut processed_sample = *sample;
                if let Some(channel_bands) = locked_filters.get_mut(channel_index) {
                    for band_filter in channel_bands {
                        processed_sample = band_filter.run(processed_sample);
                    }
                }
                *sample = processed_sample;
            }
        },
        |err| eprintln!("출력 스트림 에러: {:?}", err),
        None,
    )?;

    input_stream.play()?;
    output_stream.play()?;
    println!("\n실시간 EQ가 시작되었습니다.");
    println!("Windows 소리 설정에서 기본 출력 장치를 'CABLE Input'으로 설정했는지 확인하세요!");

    // ===================================================================
    //                        ▶▶ 메인 입력 루프 ◀◀
    // ===================================================================
    loop {
        println!("\n===== EQ 설정 변경 =====");
        println!(
            "현재 값: 저음({:.1}dB), 중음({:.1}dB), 고음({:.1}dB)",
            settings.low_shelf_gain, settings.peaking_eq_gain, settings.high_shelf_gain
        );
        println!("1. 저음(Low Shelf) Gain 변경");
        println!("2. 중음(Peaking EQ) Gain 변경");
        println!("3. 고음(High Shelf) Gain 변경");
        println!("q. 종료");
        print!("선택: ");
        std::io::stdout().flush()?;

        let mut choice = String::new();
        stdin().read_line(&mut choice)?;

        match choice.trim() {
            "1" => {
                print!("새 저음 Gain 값 (dB) 입력: ");
                std::io::stdout().flush()?;
                let mut new_gain = String::new();
                stdin().read_line(&mut new_gain)?;
                if let Ok(val) = new_gain.trim().parse::<f32>() {
                    settings.low_shelf_gain = val;
                } else {
                    println!("잘못된 값입니다.");
                    continue;
                }
            }
            "2" => {
                print!("새 중음 Gain 값 (dB) 입력: ");
                std::io::stdout().flush()?;
                let mut new_gain = String::new();
                stdin().read_line(&mut new_gain)?;
                if let Ok(val) = new_gain.trim().parse::<f32>() {
                    settings.peaking_eq_gain = val;
                } else {
                    println!("잘못된 값입니다.");
                    continue;
                }
            }
            "3" => {
                print!("새 고음 Gain 값 (dB) 입력: ");
                std::io::stdout().flush()?;
                let mut new_gain = String::new();
                stdin().read_line(&mut new_gain)?;
                if let Ok(val) = new_gain.trim().parse::<f32>() {
                    settings.high_shelf_gain = val;
                } else {
                    println!("잘못된 값입니다.");
                    continue;
                }
            }
            "q" | "Q" => {
                println!("프로그램을 종료합니다.");
                break;
            }
            _ => {
                println!("잘못된 선택입니다.");
                continue;
            }
        }

        // 설정이 변경되었으므로, 공유 필터를 업데이트합니다.
        if let Err(e) = update_filters(&filters, &settings, sample_rate, channels) {
            eprintln!("필터 업데이트 실패: {}", e);
        } else {
            println!("EQ 설정이 성공적으로 업데이트되었습니다.");
        }
    }

    Ok(())
}
