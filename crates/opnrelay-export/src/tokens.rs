// SPDX-License-Identifier: AGPL-3.0-only
//! Header/footer template tokens.

use crate::model::RenderModel;

/// Replace static tokens. `{page}` and `{pages}` are left for formats that
/// can resolve them (PDF, Word).
pub fn expand(template: &str, m: &RenderModel) -> String {
    template
        .replace("{company}", &m.branding.company)
        .replace("{guide_title}", &m.meta.title)
        .replace("{date}", &m.meta.date)
        .replace("{author}", &m.meta.author)
        .replace("{version}", &m.meta.version.to_string())
        .replace("{classification}", m.classification_label().unwrap_or(""))
}

/// For formats without pages: drop page tokens and tidy separators.
pub fn without_pages(s: &str) -> String {
    let s = s
        .replace("Page {page} of {pages}", "")
        .replace("{page}", "")
        .replace("{pages}", "");
    s.split(" · ")
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::testutil::model;

    #[test]
    fn expands_tokens() {
        let m = model();
        assert_eq!(expand("{company} - {version}", &m), "Acme Health - 3");
        assert_eq!(
            without_pages(&expand("{guide_title} · Page {page} of {pages}", &m)),
            "Add a <new> hire & more"
        );
    }
}
