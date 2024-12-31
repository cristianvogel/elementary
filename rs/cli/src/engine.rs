use std::cell::UnsafeCell;
use std::collections::{BTreeMap, HashSet, VecDeque};
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

#[derive(Serialize, Deserialize)]
pub struct Directive {
    pub graph: Option<Vec<NodeRepr>>,
}

#[cxx::bridge]
mod ffi {
    unsafe extern "C++" {
        include!("cli/src/Bindings.h");

        type RuntimeBindings;

        fn new_runtime_instance(sample_rate: f64, block_size: usize) -> UniquePtr<RuntimeBindings>;
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

    pub fn render(&mut self, roots: &Vec<NodeRepr>) -> Result<i32, &str> {
        let instructions = self.reconcile(&roots);
        self.inner.apply_instructions(&instructions)
    }

    pub fn process_queued_events(&mut self) -> Result<serde_json::Value, &str> {
        self.inner.process_queued_events()
    }
}

pub fn new_engine(sample_rate: f64, block_size: usize) -> (MainHandle, ProcessHandle) {
    let cell = UnsafeCell::new(ffi::new_runtime_instance(sample_rate, block_size));
    let arc = Arc::new(EngineInternal { inner: cell });

    (
        MainHandle::new(arc.clone()),
        ProcessHandle::new(arc.clone()),
    )
}
