//! MCP resource descriptor registration.

pub mod orchestrator;
pub mod worker;

use crate::mcp::dto::{ResourceDescriptor, ResourceTemplateDescriptor};
use crate::methodology::{MethodologyDocument, MethodologyRole};

/// Shared MCP resource descriptor builders.
///
/// Note: Resource descriptors are immutable protocol metadata, so these
/// helpers keep the repeated URI/description construction in one place.
pub(crate) struct ResourceDescriptors;

impl ResourceDescriptors {
    /// Build the methodology resource for one role.
    pub(crate) fn methodology(
        role: MethodologyRole, description: &'static str,
    ) -> ResourceDescriptor {
        ResourceDescriptor {
            uri: MethodologyDocument::new(role).resource_uri(),
            description,
            mime_type: "text/markdown",
        }
    }

    /// Build one JSON resource descriptor.
    pub(crate) const fn json(uri: &'static str, description: &'static str) -> ResourceDescriptor {
        ResourceDescriptor { uri, description, mime_type: "application/json" }
    }

    /// Build one JSON resource template descriptor.
    pub(crate) const fn template(
        uri_template: &'static str, description: &'static str,
    ) -> ResourceTemplateDescriptor {
        ResourceTemplateDescriptor { uri_template, description, mime_type: "application/json" }
    }
}
