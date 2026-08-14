use super::*;
use markup5ever_rcdom::NodeData;

pub fn normalize_markdown_from(html: &str) -> Result<String> {
    let converter = HtmlToMarkdown::new();
    let node = converter.html_to_tree(html)?;
    clean_markdown(&node);
    Ok(converter.tree_to_markdown(&node))
}

struct HtmlToMarkdown {
    converter: htmd::HtmlToMarkdown,
}

impl HtmlToMarkdown {
    fn new() -> Self {
        let converter = htmd::HtmlToMarkdown::builder()
            .skip_tags(vec![
                "script", "style", "noscript", // Executable / styling
                "iframe", "canvas", "svg", "object", "embed", // Embedded content
                "img", "picture", "source", "video", "audio", // Media
                "head", "footer", "aside", "template", "nav", // Other
            ])
            .build();
        Self { converter }
    }
}

/// Cap LLM input while keeping the head (where job listings live) and the
/// tail (deadlines / footer notes). The middle is trimmed with a marker.
pub fn cap_input(markdown: &str, max_chars: usize) -> String {
    let len = markdown.len();
    if len <= max_chars {
        return markdown.to_string();
    }

    let head = (max_chars * 3) / 4;
    let tail = max_chars - head;

    let head_end = markdown
        .char_indices()
        .nth(head)
        .map(|(i, _)| i)
        .unwrap_or(len);
    let tail_start = markdown
        .char_indices()
        .rev()
        .nth(tail)
        .map(|(i, _)| i)
        .unwrap_or(0);

    let trimmed = len.saturating_sub(head + tail);
    format!(
        "{}\n\n…[{trimmed} chars trimmed]…\n\n{}",
        &markdown[..head_end],
        &markdown[tail_start..]
    )
}

pub fn is_empty(node: &htmd::Node) -> bool {
    node.children
        .borrow()
        .iter()
        .all(|child| match &child.data {
            NodeData::Text { contents } => contents.borrow().trim().is_empty(),
            NodeData::Comment { .. } => true,
            NodeData::Element { .. } => is_empty(child),
            _ => false,
        })
}

pub fn clean_markdown(node: &htmd::Node) {
    node.children.borrow_mut().retain(|child| {
        match &child.data {
            NodeData::Comment { .. } => false,
            NodeData::Text { contents } => !contents.borrow().trim().is_empty(),
            NodeData::Element { attrs, .. } => {
                if attrs
                    .borrow()
                    .iter()
                    .find(|attr| attr.name.local.as_ref() == "href")
                    .map(|attr| attr.value.as_ref())
                    .is_some_and(|link| link.is_empty() || link.trim().starts_with('#'))
                {
                    return false;
                }

                if is_empty(child) {
                    // Remove empty items.
                    return false;
                }

                true
            }
            _ => true,
        }
    });

    for child in node.children.borrow().iter() {
        clean_markdown(child);
    }
}
