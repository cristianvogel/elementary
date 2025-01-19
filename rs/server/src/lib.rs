use elem::{engine::AudioBuffer, node::NodeRepr};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Default)]
pub struct UnresolvedDirective {
    pub graph: Option<Vec<NodeRepr>>,
    pub resources: Option<HashMap<String, String>>,
}

fn decode_audio_data(data: &Vec<u8>) -> Option<AudioBuffer<f32>> {
    use hound;

    let mut reader = hound::WavReader::new(data.as_slice()).unwrap();
    let bit_depth = reader.spec().bits_per_sample as f64;
    dbg!(reader.spec().sample_rate);
    let interleaved_buffer = reader
        .samples::<i32>()
        .map(|x| x.unwrap() as f64 / (2.0f64.powf(bit_depth) - 1.0))
        .collect::<Vec<f64>>();
    let num_channels = reader.spec().channels as usize;
    let num_frames = (reader.len() as usize) / num_channels;

    Some(AudioBuffer::<f32> {
        data: interleaved_buffer
            .into_iter()
            .map(|x| x as f32)
            .collect::<Vec<f32>>(),
        channels: num_channels,
        frames: num_frames,
    })
}

async fn resolve_resources(
    resources: &HashMap<String, String>,
) -> HashMap<String, AudioBuffer<f32>> {
    let mut result = HashMap::new();

    for (name, path) in resources.iter() {
        if let Ok(contents) = tokio::fs::read(path).await {
            let _ = result.insert(name.clone(), decode_audio_data(&contents).unwrap());
        }
    }

    result
}

pub async fn resolve_directive(directive: UnresolvedDirective) -> elem::engine::Directive {
    elem::engine::Directive {
        graph: directive.graph,
        resources: match directive.resources {
            None => None,
            Some(rs) => Some(resolve_resources(&rs).await),
        },
    }
}
