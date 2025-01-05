use std::cell::UnsafeCell;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Serialize, Deserialize)]
pub struct NodeRepr {
    hash: i32,
    kind: String,
    props: serde_json::Map<String, serde_json::Value>,
    output_channel: u32,
    children: Vec<NodeRepr>,
}

fn create_node(
    kind: &str,
    props: serde_json::Map<String, serde_json::Value>,
    children: Vec<NodeRepr>,
) -> NodeRepr {
    let mut hasher = DefaultHasher::new();

    kind.hash(&mut hasher);
    props.hash(&mut hasher);

    for child in children.iter() {
        child.hash.hash(&mut hasher);
    }

    NodeRepr {
        hash: hasher.finish() as i32,
        kind: kind.to_string(),
        props,
        output_channel: 0,
        children,
    }
}

fn root(x: NodeRepr) -> NodeRepr {
    create_node(
        "root",
        serde_json::json!({"channel": 0.0})
            .as_object()
            .unwrap()
            .clone(),
        vec![x],
    )
}

fn sin(x: NodeRepr) -> NodeRepr {
    create_node("sin", Default::default(), vec![x])
}

fn mul2(x: NodeRepr, y: NodeRepr) -> NodeRepr {
    create_node("mul", Default::default(), vec![x, y])
}

fn phasor(rate: NodeRepr) -> NodeRepr {
    create_node("phasor", Default::default(), vec![rate])
}

#[derive(Serialize, Deserialize)]
struct ConstNodeProps {
    key: Option<String>,
    value: f64,
}

fn constant(props: &ConstNodeProps) -> NodeRepr {
    create_node(
        "const",
        serde_json::to_value(&props)
            .unwrap()
            .as_object()
            .unwrap()
            .clone(),
        vec![],
    )
}

macro_rules! constant {
    // Match the macro pattern with a key-value pair in the first argument
    ({$($key:ident: $value:expr),*}) => {
        {
            // Create the props struct with the provided key-value pairs
            let props = ConstNodeProps { $($key: $value),* };

            // Call the constant function with the constructed props
            constant(&props)
        }
    };
}

struct ShallowNodeRepr {
    hash: i32,
    kind: String,
    props: serde_json::Map<String, serde_json::Value>,
    output_channel: u32,
    children: Vec<i32>,
}

fn shallow_clone(node: &NodeRepr) -> ShallowNodeRepr {
    ShallowNodeRepr {
        hash: node.hash,
        kind: node.kind.clone(),
        props: node.props.clone(),
        output_channel: node.output_channel,
        children: node.children.iter().map(|n| n.hash).collect::<Vec<i32>>(),
    }
}

#[derive(Serialize, Deserialize, Default)]
pub struct Directive {
    pub graph: Option<Vec<NodeRepr>>,
    pub resources: Option<HashMap<String, String>>,
}

pub trait FloatType: 'static {}
impl FloatType for f32 {}
impl FloatType for f64 {}

#[derive(Default)]
pub struct AudioBuffer<T> {
    data: Vec<T>,
    channels: usize,
    frames: usize,
}

impl<T> AudioBuffer<T>
where
    T: FloatType + Clone + Default,
{
    pub fn new(channels: usize, frames: usize) -> Self {
        Self {
            data: vec![Default::default(); channels * frames],
            channels,
            frames,
        }
    }
}

pub struct ResolvedDirective {
    pub graph: Option<Vec<NodeRepr>>,
    pub resources: Option<HashMap<String, AudioBuffer<f32>>>,
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

    // Hmm... I'm confused. The Hound docs suggest that the data is interleaved,
    // but it doesn't actually appear to be.
    // let mut deinterleaved = Vec::new();
    // deinterleaved.resize(interleaved_buffer.len(), 0.0f32);

    // for i in 0..num_channels {
    //     for j in 0..num_frames {
    //         deinterleaved[i * num_frames + j] = interleaved_buffer[i + j * num_channels] as f32;
    //     }
    // }

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

pub async fn resolve_directive(directive: Directive) -> ResolvedDirective {
    ResolvedDirective {
        graph: directive.graph,
        resources: match directive.resources {
            None => None,
            Some(rs) => Some(resolve_resources(&rs).await),
        },
    }
}

#[cxx::bridge]
mod ffi {
    unsafe extern "C++" {
        include!("elem/src/Bindings.h");

        type RuntimeBindings;

        fn new_runtime_instance(sample_rate: f64, block_size: usize) -> UniquePtr<RuntimeBindings>;
        fn add_shared_resource(
            self: Pin<&mut RuntimeBindings>,
            name: &String,
            channels: usize,
            frames: usize,
            data: &[f32],
        ) -> i32;
        fn apply_instructions(self: Pin<&mut RuntimeBindings>, batch: &String) -> i32;
        fn process_queued_events(self: Pin<&mut RuntimeBindings>) -> String;

        unsafe fn process(
            self: Pin<&mut RuntimeBindings>,
            input_data: *const f32,
            output_data: *mut f32,
            num_channels: usize,
            num_frames: usize,
        ) -> ();
    }
}

struct EngineInternal {
    inner: UnsafeCell<cxx::UniquePtr<ffi::RuntimeBindings>>,
}

unsafe impl Send for EngineInternal {}
unsafe impl Sync for EngineInternal {}

impl EngineInternal {
    pub fn add_shared_resource(
        &self,
        name: &String,
        channels: usize,
        frames: usize,
        data: &[f32],
    ) -> i32 {
        unsafe {
            self.inner
                .get()
                .as_mut()
                .unwrap()
                .as_mut()
                .unwrap()
                .add_shared_resource(name, channels, frames, data)
        }
    }

    pub fn apply_instructions(&self, instructions: &serde_json::Value) -> Result<i32, &str> {
        unsafe {
            let result = self
                .inner
                .get()
                .as_mut()
                .unwrap()
                .as_mut()
                .unwrap()
                .apply_instructions(&instructions.to_string());

            Ok(result)
        }
    }

    pub fn process_queued_events(&self) -> Result<serde_json::Value, &str> {
        unsafe {
            let batch = self
                .inner
                .get()
                .as_mut()
                .unwrap()
                .as_mut()
                .unwrap()
                .process_queued_events();

            Ok(serde_json::from_str(&batch).unwrap())
        }
    }
}

pub struct ProcessHandle {
    inner: Arc<EngineInternal>,
}

impl ProcessHandle {
    pub fn new(inner: Arc<EngineInternal>) -> Self {
        Self { inner }
    }

    pub fn process(
        &self,
        input_data: *const f32,
        output_data: *mut f32,
        num_channels: usize,
        num_frames: usize,
    ) {
        unsafe {
            self.inner
                .inner
                .get()
                .as_mut()
                .unwrap()
                .as_mut()
                .unwrap()
                .process(input_data, output_data, num_channels, num_frames);
        }
    }
}

pub struct MainHandle {
    inner: Arc<EngineInternal>,
    node_map: BTreeMap<i32, ShallowNodeRepr>,
}

impl MainHandle {
    pub fn new(inner: Arc<EngineInternal>) -> Self {
        Self {
            inner: inner,
            node_map: BTreeMap::new(),
        }
    }

    pub fn reconcile(&mut self, roots: &Vec<NodeRepr>) -> serde_json::Value {
        let mut visited: HashSet<i32> = HashSet::new();
        let mut queue: VecDeque<&NodeRepr> = VecDeque::new();
        let mut instructions = serde_json::Value::Array(vec![]);

        for root in roots.iter() {
            // TODO: ref?
            queue.push_back(root);
        }

        while !queue.is_empty() {
            let next = queue.pop_front().unwrap();

            if visited.contains(&next.hash) {
                continue;
            }

            // Mount
            if !self.node_map.contains_key(&next.hash) {
                // Create node
                instructions
                    .as_array_mut()
                    .unwrap()
                    .push(json!([0, next.hash, next.kind]));

                // Append child
                for child in next.children.iter() {
                    instructions.as_array_mut().unwrap().push(json!([
                        2,
                        next.hash,
                        child.hash,
                        child.output_channel
                    ]));
                }

                self.node_map.insert(next.hash, shallow_clone(&next));
            }

            // Props
            for (name, value) in &next.props {
                // TODO: Only add the instruction if the prop value != existing prop value
                instructions
                    .as_array_mut()
                    .unwrap()
                    .push(json!([3, next.hash, name, value]));
            }

            for child in next.children.iter() {
                queue.push_back(child);
            }

            visited.insert(next.hash);
        }

        // Activate roots
        instructions.as_array_mut().unwrap().push(json!([
            4,
            roots.iter().map(|n| n.hash).collect::<Vec<i32>>()
        ]));

        // Commit
        instructions.as_array_mut().unwrap().push(json!([5]));

        // Sort so that creates land before appends, etc
        instructions
            .as_array_mut()
            .unwrap()
            .sort_by(|a, b| a[0].as_i64().cmp(&b[0].as_i64()));

        instructions
    }

    pub fn render(&mut self, directive: ResolvedDirective) -> Result<i32, &str> {
        if let Some(resources) = directive.resources {
            for (k, v) in resources.into_iter() {
                let rc =
                    self.inner
                        .add_shared_resource(&k, v.channels, v.frames, v.data.as_slice());
                println!("Add resource result: {}", rc);
            }
        }

        if let Some(graph) = directive.graph {
            let instructions = self.reconcile(&graph);
            let result = self.inner.apply_instructions(&instructions);
            println!("Apply instructions result: {}", result.unwrap_or(-1));

            result
        } else {
            Ok(0)
        }
    }

    pub fn process_queued_events(&mut self) -> Result<serde_json::Value, &str> {
        self.inner.process_queued_events()
    }
}

pub fn new_engine(sample_rate: f64, block_size: usize) -> (MainHandle, ProcessHandle) {
    let cell = UnsafeCell::new(ffi::new_runtime_instance(sample_rate, block_size));
    let arc = Arc::new(EngineInternal { inner: cell });

    let mut main = MainHandle::new(arc.clone());
    let proc = ProcessHandle::new(arc.clone());

    let cycle = root(sin(mul2(
        constant!({key: None, value: 2.0 * std::f64::consts::PI}),
        phasor(constant!({key: None, value: 110.0})),
    )));
    let roots = vec![cycle];

    let _ = main.render(ResolvedDirective {
        graph: Some(roots),
        resources: None,
    });

    (main, proc)
}
