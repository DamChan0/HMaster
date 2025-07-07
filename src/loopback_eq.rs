use anyhow::Result;
use clap::Parser;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use crate::equalization::BiquadEq;

/// 루프백 스트림 생성기
pub struct LoopbackWithEq {
    input_stream: cpal::Stream,
    output_stream: cpal::Stream,
}

impl LoopbackWithEq {
    /// 새로운 LoopbackWithEq 생성
    /// - `input_device`, `output_device`: 캡처 및 재생 장치
    /// - `volume`: 기본 볼륨 배수
    /// - `eq_filter`: 이퀄라이저 필터(Optional)
    ///
    ///
    pub fn new(
        input_device: cpal::Device,
        output_device: cpal::Device,
        volume: f32,
    ) -> Result<Self> {
        // 스트림 설정
        let in_config: cpal::StreamConfig = input_device.default_input_config()?.into();
        let out_config: cpal::StreamConfig = output_device.default_output_config()?.into();
        println!("Input config: {:?}", in_config);
        println!("Output config: {:?}", out_config);

        let eq_filter = Some(Arc::new(BiquadEq::new(
            in_config.sample_rate.0 as f32,
            15.0,  // low gain in dB
            0.0,   // mid gain
            -10.0, // high gain
        )));
        // 공유 버퍼
        let buffer = Arc::new(Mutex::new(VecDeque::<f32>::new()));
        let prod_buf = buffer.clone();
        let cons_buf = buffer.clone();
        let channels = in_config.channels as usize;
        // 입력 스트림 콜백
        let input_stream = input_device.build_input_stream(
            &in_config,
            move |data: &[f32], _| {
                let mut buf = prod_buf.lock().unwrap();
                for frame in data.chunks(channels) {
                    for &sample in frame {
                        let raw = sample * volume;
                        let processed = if let Some(ref eq) = eq_filter {
                            eq.process(raw)
                        } else {
                            raw
                        };

                        if raw.abs() > 1e-4 || (processed - raw).abs() > 1e-4 {
                            // println!("[DEBUG_IN] raw={:.5}, proc={:.5}", raw, processed);
                        }

                        buf.push_back(sample);
                    }
                }
            },
            move |err| eprintln!("입력 스트림 에러: {}", err),
            None,
        )?;

        // 출력 스트림 콜백
        let output_stream = output_device.build_output_stream(
            &out_config,
            move |out: &mut [f32], _| {
                let mut buf = cons_buf.lock().unwrap();
                for slot in out.iter_mut() {
                    *slot = buf.pop_front().unwrap_or(0.0);
                }
            },
            move |err| eprintln!("출력 스트림 에러: {}", err),
            None,
        )?;

        Ok(Self {
            input_stream,
            output_stream,
        })
    }

    /// 스트림 재생 시작
    pub fn play(&self) -> Result<()> {
        self.input_stream.play()?;
        self.output_stream.play()?;
        Ok(())
    }
}
