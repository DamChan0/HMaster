use anyhow::{Result, anyhow};
use biquad::{Biquad, Coefficients, DirectForm1, Hertz, Q_BUTTERWORTH_F32, ToHertz, Type};
use cpal::StreamConfig;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::collections::VecDeque;
use std::io;
use std::process::Command;
use std::sync::{Arc, Mutex};

#[derive(Debug, thiserror::Error)]
pub enum EqError {
    #[error("루프백 입력 장치 없음")]
    NoLoopbackInput,
    #[error("장치 에러: {0}")]
    DeviceError(#[from] cpal::DevicesError),
    #[error("기타 에러: {0}")]
    Other(#[from] std::io::Error),
    #[error("출력 장치 없음")]
    NoOutputDevice,
}

#[cfg(target_os = "linux")]
pub fn ensure_linux_loopback_loaded() -> io::Result<u32> {
    let output = Command::new("pactl")
        .args(&["load-module", "module-loopback", "latency_msec=1"])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            "루프백 모듈 로드 실패",
        ));
    }
    let id = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<u32>()
        .unwrap_or(0);
    Ok(id)
}

#[cfg(target_os = "linux")]
pub fn unload_linux_loopback(id: u32) {
    let _ = Command::new("pactl")
        .args(&["unload-module", &id.to_string()])
        .output();
}

#[cfg(target_os = "windows")]
pub fn ensure_windows_loopback_setup() {
    // Windows에서 별도 로딩 불필요: CPAL이 Loopback/Stereo Mix 자동 처리
}

pub struct BiquadEq {
    low: Mutex<DirectForm1<f32>>,
    mid: Mutex<DirectForm1<f32>>,
    high: Mutex<DirectForm1<f32>>,
}

impl BiquadEq {
    pub fn new(sample_rate: f32, low_gain: f32, mid_gain: f32, high_gain: f32) -> Self {
        let sr = sample_rate.hz();
        // let min = 20.hz(); // 이 변수들은 이제 Coefficients::from_params에 직접 전달되지 않습니다.
        // let max: Hertz<f32> = 20_000.hz(); // 이 변수들은 이제 Coefficients::from_params에 직접 전달되지 않습니다.

        // Low-shelf 필터
        // Type::LowShelf는 게인만 받습니다.
        // from_params의 3번째 인자는 차단 주파수(Hertz<f32>), 4번째 인자는 슬로프(f32)입니다.
        let low_shelf_freq = 250.0.hz(); // 저역대 쉘프 필터의 차단 주파수 (예시)
        let low_shelf_slope = 0.5; // 저역대 쉘프 필터의 슬로프 (예시)
        let low = Coefficients::<f32>::from_params(
            Type::LowShelf(low_gain),
            sr,
            low_shelf_freq,
            low_shelf_slope,
        )
        .unwrap();

        // Peaking 필터
        // Type::Peaking은 freq, gain, q를 포함합니다.
        // from_params의 3번째 인자는 피크 주파수(Hertz<f32>), 4번째 인자는 Q-팩터(f32)입니다.
        // Type 안에 있는 정보와 중복되지만, biquad 라이브러리가 명시적인 4개의 인자를 요구합니다.
        let mid_peak_freq = 300.0.hz();
        let mid_q_factor = Q_BUTTERWORTH_F32;
        let mid = Coefficients::<f32>::from_params(
            Type::PeakingEQ(mid_gain), // 여기가 PeakingEQ로 변경되었습니다.
            sr,
            mid_peak_freq,
            mid_q_factor,
        )
        .unwrap();

        // High-shelf 필터
        // Type::HighShelf는 freq, gain, slope를 포함합니다.
        // from_params의 3번째 인자는 차단 주파수(Hertz<f32>), 4번째 인자는 슬로프(f32)입니다.
        // Type 안에 있는 정보와 중복되지만, biquad 라이브러리가 명시적인 4개의 인자를 요구합니다.
        let high_shelf_freq = 12000.0.hz();
        let high_shelf_slope = 0.5;
        let high = Coefficients::<f32>::from_params(
            Type::HighShelf(high_gain),
            sr,
            high_shelf_freq,
            high_shelf_slope,
        )
        .unwrap();

        Self {
            low: Mutex::new(DirectForm1::new(low)),
            mid: Mutex::new(DirectForm1::new(mid)),
            high: Mutex::new(DirectForm1::new(high)),
        }
    }

    pub fn process(&self, sample: f32) -> f32 {
        let low = self.low.lock().unwrap().run(sample);
        let mid = self.mid.lock().unwrap().run(low);
        self.high.lock().unwrap().run(mid)
    }
}

pub fn select_loopback_input_device() -> Result<cpal::Device, EqError> {
    let host = cpal::default_host();

    let device = host
        .input_devices()?
        .find(|d| {
            let name = d.name().unwrap_or_default().to_lowercase();
            name.contains("monitor")
                || name.contains("loopback")
                || name.contains("stereo mix")
                || name.contains("스테레오 믹스")
        })
        .ok_or(EqError::NoLoopbackInput)?;
    Ok(device)
}

pub fn select_input_or_default() -> Result<cpal::Device, EqError> {
    match select_loopback_input_device() {
        Ok(dev) => Ok(dev),
        Err(EqError::NoLoopbackInput) => {
            let host = cpal::default_host();
            host.default_input_device().ok_or(EqError::NoLoopbackInput)
        }
        Err(e) => Err(e),
    }
}

/// 기본 출력 장치 선택
pub fn select_output_device() -> Result<cpal::Device, EqError> {
    let host = cpal::default_host();
    host.default_output_device().ok_or(EqError::NoOutputDevice)
}

/// 장치의 default_input_config를 StreamConfig로 변환
pub fn to_stream_config_input(device: &cpal::Device) -> Result<StreamConfig> {
    Ok(device.default_input_config()?.into())
}

/// 장치의 default_output_config를 StreamConfig로 변환
pub fn to_stream_config_output(device: &cpal::Device) -> Result<StreamConfig> {
    Ok(device.default_output_config()?.into())
}
// Equalizer 모듈은 이 함수들로 입출력 장치와 설정, EQ 필터를 제공하며
// 스트림 생성 및 재생은 loopback.rs에서 수행하세요.
