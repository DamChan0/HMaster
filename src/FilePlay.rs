use rodio::{Decoder, OutputStream, Sink, Source};
use std::error::Error;
use std::fs::File;
use std::io::BufReader;

pub fn play_file(file_path: &str, volume: f32) -> Result<(), Box<dyn Error>> {
    // OutputStream을 생성합니다.
    let (stream, stream_handle) = OutputStream::try_default()?;

    let sink = Sink::try_new(&stream_handle)?;
    sink.set_volume(volume);

    let file = File::open(file_path)?;
    let source = Decoder::new(BufReader::new(file))?;
    sink.append(source);

    sink.sleep_until_end();
    // stream이 여기서 drop 되며 자원 해제
    drop(stream);
    Ok(())
}
