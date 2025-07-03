use anyhow::{Ok, Result};
use clap::Parser; // Command-line argument parsing
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

pub struct Loopback {
    input_stream: cpal::Stream,
    output_stream: cpal::Stream,
}
impl Loopback {
    pub fn new(
        input_device: cpal::Device,
        output_device: cpal::Device,
        volume: f32,
    ) -> Result<Self> {
        // 1) 입력/출력 스트림 설정
        let out_config: cpal::StreamConfig = output_device.default_output_config()?.into();
        // let in_config = cpal::StreamConfig {
        //     channels: out_config.channels,
        //     sample_rate: out_config.sample_rate,
        //     buffer_size: out_config.buffer_size.clone(),
        // };
        let in_config: cpal::StreamConfig = input_device.default_input_config()?.into();

        for config in input_device.supported_input_configs()? {
            println!("Input config: {:?}", config);
        }
        for config in output_device.supported_output_configs()? {
            println!("Output config: {:?}", config);
        }

        // 2) 공유 버퍼: VecDeque<f32>
        let buffer = Arc::new(Mutex::new(VecDeque::<f32>::new()));
        let prod_buf = Arc::clone(&buffer);
        let cons_buf = Arc::clone(&buffer);

        let input_stream = input_device.build_input_stream(
            &in_config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                let mut buf = prod_buf.lock().unwrap();
                for &s in data {
                    buf.push_back(s as f32 * volume);
                    // 무한 증가 방지: 1초치 이상은 버리기
                    if buf.len() > (in_config.sample_rate.0 as usize * in_config.channels as usize)
                    {
                        buf.pop_front();
                    }
                }
            },
            move |err| eprintln!("입력 스트림 에러: {}", err),
            None,
        )?;

        // 4) 출력 스트림: 버퍼에서 샘플을 꺼내 플레이
        let output_stream = output_device.build_output_stream(
            &out_config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                let mut buf = cons_buf.lock().unwrap();
                for out_sample in data.iter_mut() {
                    if let Some(s) = buf.pop_front() {
                        *out_sample = s;
                        // print!("{} ", *out_sample);
                    } else {
                        *out_sample = 0.0;
                    }
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

    pub fn play(&self) -> Result<()> {
        self.input_stream.play()?;
        self.output_stream.play()?;
        Ok(())
    }
}
