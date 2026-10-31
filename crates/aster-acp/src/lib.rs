use std::collections::VecDeque;
use std::sync::Mutex;

use agent_client_protocol::schema::v1::{ContentBlock, EmbeddedResourceResource};
use aster_policy::Mode;

const MODES: [(Mode, &str, &str, &str); 5] = [
    (
        Mode::Plan,
        "plan",
        "Plan",
        "Explore the code and present a plan before editing",
    ),
    (
        Mode::Manual,
        "manual",
        "Manual",
        "Ask for approval before each edit and command",
    ),
    (
        Mode::Auto,
        "auto",
        "Auto",
        "Apply edits and run commands, pausing on the risky ones",
    ),
    (
        Mode::Edit,
        "edit",
        "Edit",
        "As auto, but commands are trusted; only a rule stops one",
    ),
    (
        Mode::Yolo,
        "yolo",
        "Yolo",
        "Skip the rules and isolation entirely. Use with extreme caution",
    ),
];

pub fn modes() -> impl Iterator<Item = (Mode, &'static str, &'static str, &'static str)> {
    MODES.into_iter()
}

pub fn mode_id(mode: Mode) -> &'static str {
    MODES
        .iter()
        .find(|(candidate, ..)| *candidate == mode)
        .map_or("auto", |(_, id, ..)| id)
}

pub fn mode_from_id(id: &str) -> Option<Mode> {
    MODES
        .iter()
        .find(|(_, mode_id, ..)| *mode_id == id)
        .map(|(mode, ..)| *mode)
}

/// An image a prompt carries, still base64 as the client sent it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromptImage {
    pub data: String,
    pub mime_type: String,
    pub uri: Option<String>,
}

/// A prompt's text and the images it carries, in the order the client sent them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Prompt {
    pub text: String,
    pub images: Vec<PromptImage>,
}

/// Flatten the blocks into a turn: text and resource context as text, images
/// kept aside so the caller can put them on the wire as image parts.
pub fn prompt_content(blocks: &[ContentBlock]) -> Prompt {
    let mut prompt = Prompt::default();
    let mut context = Vec::new();
    for block in blocks {
        match block {
            ContentBlock::Text(value) => prompt.text.push_str(&value.text),
            ContentBlock::Image(image) => prompt.images.push(PromptImage {
                data: image.data.clone(),
                mime_type: image.mime_type.clone(),
                uri: image.uri.clone(),
            }),
            ContentBlock::ResourceLink(link) => {
                let uri = link.uri.strip_prefix("file://").unwrap_or(&link.uri);
                prompt.text.push_str(&format!("[{}]({uri})", link.name));
            }
            ContentBlock::Resource(resource) => {
                if let EmbeddedResourceResource::TextResourceContents(contents) = &resource.resource
                {
                    let uri = contents
                        .uri
                        .strip_prefix("file://")
                        .unwrap_or(&contents.uri);
                    context.push(format!(
                        "<context path=\"{uri}\">\n{}\n</context>",
                        contents.text
                    ));
                }
            }
            _ => {}
        }
    }
    if !context.is_empty() {
        prompt.text.push_str("\n\n");
        prompt.text.push_str(&context.join("\n\n"));
    }
    prompt
}

#[derive(Clone)]
pub struct Call {
    pub id: String,
    pub name: String,
}

#[derive(Default)]
pub struct Calls {
    queue: Mutex<VecDeque<Call>>,
}

impl Calls {
    pub fn push(&self, call: Call) {
        if let Ok(mut queue) = self.queue.lock() {
            queue.push_back(call);
        }
    }

    pub fn finish(&self, id: &str) {
        if let Ok(mut queue) = self.queue.lock() {
            queue.retain(|call| call.id != id);
        }
    }

    pub fn current(&self) -> Option<Call> {
        self.queue.lock().ok()?.front().cloned()
    }

    pub fn name_of(&self, id: &str) -> Option<String> {
        let queue = self.queue.lock().ok()?;
        queue
            .iter()
            .find(|call| call.id == id)
            .map(|call| call.name.clone())
    }
}

pub fn crlf(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\n', "\r\n")
}

#[cfg(test)]
#[path = "core_tests.rs"]
mod tests;
