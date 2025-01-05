use crate::node::{create_node, NodeRepr};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub fn root(x: NodeRepr) -> NodeRepr {
    create_node(
        "root",
        json!({"channel": 0.0}).as_object().unwrap().clone(),
        vec![x],
    )
}

pub fn sin(x: NodeRepr) -> NodeRepr {
    create_node("sin", Default::default(), vec![x])
}

pub fn mul2(x: NodeRepr, y: NodeRepr) -> NodeRepr {
    create_node("mul", Default::default(), vec![x, y])
}

pub fn phasor(rate: NodeRepr) -> NodeRepr {
    create_node("phasor", Default::default(), vec![rate])
}

#[derive(Serialize, Deserialize)]
pub struct ConstNodeProps {
    pub key: Option<String>,
    pub value: f64,
}

pub fn constant(props: &ConstNodeProps) -> NodeRepr {
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

#[macro_export]
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

pub use crate::constant;
