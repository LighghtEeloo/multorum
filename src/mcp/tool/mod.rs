//! MCP tool descriptor registration.

pub mod orchestrator;
pub mod worker;

use crate::mcp::dto::{ToolInputDescriptor, ToolInputType};

/// Construct one required string tool input descriptor.
pub(crate) const fn required_string_input(
    name: &'static str, description: &'static str,
) -> ToolInputDescriptor {
    ToolInputDescriptor::required(name, description, ToolInputType::String)
}

/// Construct one optional string tool input descriptor.
pub(crate) const fn optional_string_input(
    name: &'static str, description: &'static str,
) -> ToolInputDescriptor {
    ToolInputDescriptor::optional(name, description, ToolInputType::String)
}

/// Construct one required integer tool input descriptor.
pub(crate) const fn required_integer_input(
    name: &'static str, description: &'static str,
) -> ToolInputDescriptor {
    ToolInputDescriptor::required(name, description, ToolInputType::Integer)
}

/// Construct one optional integer tool input descriptor.
pub(crate) const fn optional_integer_input(
    name: &'static str, description: &'static str,
) -> ToolInputDescriptor {
    ToolInputDescriptor::optional(name, description, ToolInputType::Integer)
}

/// Construct one optional boolean tool input descriptor.
pub(crate) const fn optional_boolean_input(
    name: &'static str, description: &'static str,
) -> ToolInputDescriptor {
    ToolInputDescriptor::optional(name, description, ToolInputType::Boolean)
}

/// Construct one required string-list tool input descriptor.
pub(crate) const fn required_string_list_input(
    name: &'static str, description: &'static str,
) -> ToolInputDescriptor {
    ToolInputDescriptor::required(name, description, ToolInputType::StringList)
}

/// Construct one optional string-list tool input descriptor.
pub(crate) const fn optional_string_list_input(
    name: &'static str, description: &'static str,
) -> ToolInputDescriptor {
    ToolInputDescriptor::optional(name, description, ToolInputType::StringList)
}
