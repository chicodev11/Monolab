
use pulldown_cmark::{html, Options, Parser};

pub fn render(markdown: &str) -> String {
    if markdown.trim().is_empty() {
        return String::new();
    }
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    let mut raw = String::new();
    html::push_html(&mut raw, Parser::new_ext(markdown, options));
    let mut builder = ammonia::Builder::default();
    builder
        .add_tags(["img", "details", "summary", "center", "sub", "sup"])
        .add_tag_attributes("img", ["src", "alt", "title", "width", "height", "align"])
        .add_tag_attributes("details", ["open"])
        .add_generic_attributes(["align"])
        .url_schemes(["http", "https", "mailto"].into_iter().collect())
        .link_rel(Some("noopener noreferrer"));
    builder.clean(&raw).to_string()
}

#[cfg(test)]
mod tests {
    use super::render;

    #[test]
    fn strips_scripts_but_keeps_pictures_and_links() {
        let html = render(
            "# Title\n\n<script>alert(1)</script><img src=\"https://cdn.modrinth.com/a.png\" onerror=\"alert(2)\">\n\n[site](javascript:alert(3)) [docs](https://example.com)\n\n| a | b |\n|---|---|\n| 1 | 2 |\n",
        );
        assert!(html.contains("<h1>Title</h1>"), "{html}");
        assert!(!html.contains("<script"), "{html}");
        assert!(!html.contains("onerror"), "{html}");
        assert!(!html.contains("javascript:"), "{html}");
        assert!(html.contains("<img src=\"https://cdn.modrinth.com/a.png\""), "{html}");
        assert!(html.contains("href=\"https://example.com\""), "{html}");
        assert!(html.contains("<table>"), "{html}");
    }

    #[test]
    fn empty_stays_empty() {
        assert_eq!(render("  \n"), "");
    }
}
