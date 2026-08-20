//! The document's stylesheets as CSSOM addresses them.
//!
//! `StyleSheetSet` in silksurf-app derives the document's sheet list by walking
//! the DOM and hands the cascade one concatenated text. Script addresses a
//! sheet instead: `document.styleSheets[i]`, `styleElement.sheet`, and the
//! `insertRule` that Emotion drives all name one sheet and splice its rules.
//! `SheetSet` is that addressable form -- one `Stylesheet` per source, kept
//! beside the owner node and media the source carries, mutable by index.
//!
//! Both the app runtime and `SilkContext` hold the same set, so a rule script
//! inserts is a rule the next `StyleIndex` build reads. `script_generation`
//! moves on every scripted mutation, which is the signal the repaint tick
//! drains: an `insertRule` call touches no DOM node, so neither
//! `Dom::structure_generation` nor `Dom::generation` reports it.

use silksurf_core::SilkInterner;
use silksurf_dom::NodeId;

use crate::parser::{Rule, Stylesheet, parse_stylesheet};
use crate::selector::intern_rules;
use crate::serialize::{declarations_to_css, rule_to_css, selector_list_to_css};

/// Why a CSSOM mutation was refused, named for the `DOMException` it raises.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SheetError {
    /// The rule index is past the end of the rule list. CSSOM raises
    /// `IndexSizeError`.
    IndexSize,
    /// The text does not parse as exactly one rule. CSSOM raises
    /// `SyntaxError`.
    Syntax,
    /// No sheet carries this index.
    NoSuchSheet,
}

/// Where a sheet's rules come from, which decides whether script sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SheetOrigin {
    /// The engine's default presentation rules. CSSOM omits them: a page
    /// enumerating `document.styleSheets` reads its own sheets alone.
    UserAgent,
    /// A `<style>` element or a `<link rel=stylesheet>` the document carries.
    Author,
}

/// One sheet in the document's ordered list.
#[derive(Debug, Clone)]
pub struct LiveSheet {
    /// The `<style>` or `<link>` element the sheet came from, which
    /// `CSSStyleSheet.ownerNode` returns and which Emotion matches against the
    /// tag it inserted.
    pub owner: Option<NodeId>,
    /// The resolved URL for a link sheet, which `CSSStyleSheet.href` returns.
    pub href: Option<String>,
    /// The `media` attribute, empty when the sheet applies unconditionally.
    pub media: String,
    /// A disabled sheet stays in the list and contributes no rules.
    pub disabled: bool,
    pub origin: SheetOrigin,
    pub rules: Stylesheet,
    /// Set when script splices this sheet, cleared once the rules carry
    /// interned atoms. A rule parsed without the document's interner matches
    /// by string alone until `intern_scripted` runs.
    scripted: bool,
}

impl LiveSheet {
    #[must_use]
    pub fn new(origin: SheetOrigin, rules: Stylesheet) -> Self {
        Self {
            owner: None,
            href: None,
            media: String::new(),
            disabled: false,
            origin,
            rules,
            scripted: false,
        }
    }

    #[must_use]
    pub fn with_owner(mut self, owner: NodeId) -> Self {
        self.owner = Some(owner);
        self
    }

    #[must_use]
    pub fn with_href(mut self, href: Option<String>) -> Self {
        self.href = href;
        self
    }

    #[must_use]
    pub fn with_media(mut self, media: String) -> Self {
        self.media = media;
        self
    }
}

/// The document's sheets, addressable by index and mutable from script.
#[derive(Debug, Default)]
pub struct SheetSet {
    sheets: Vec<LiveSheet>,
    script_generation: u64,
}

impl SheetSet {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace the whole list, which is what a DOM-visible stylesheet change
    /// produces: the source walk reruns and every sheet reparses.
    pub fn replace(&mut self, sheets: Vec<LiveSheet>) {
        self.sheets = sheets;
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.sheets.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sheets.is_empty()
    }

    #[must_use]
    pub fn sheets(&self) -> &[LiveSheet] {
        &self.sheets
    }

    /// The rules the cascade reads, in list order, skipping disabled sheets.
    pub fn active_sheets(&self) -> impl Iterator<Item = &Stylesheet> {
        self.sheets
            .iter()
            .filter(|sheet| !sheet.disabled)
            .map(|sheet| &sheet.rules)
    }

    /// The list positions script enumerates, which omits the user-agent sheet.
    #[must_use]
    pub fn author_indices(&self) -> Vec<usize> {
        self.sheets
            .iter()
            .enumerate()
            .filter(|(_, sheet)| sheet.origin == SheetOrigin::Author)
            .map(|(index, _)| index)
            .collect()
    }

    /// The list position of the sheet a `<style>` or `<link>` element owns,
    /// which is how `HTMLStyleElement.sheet` resolves.
    #[must_use]
    pub fn index_of_owner(&self, owner: NodeId) -> Option<usize> {
        self.sheets
            .iter()
            .position(|sheet| sheet.owner == Some(owner))
    }

    /// Moves on every scripted splice. The repaint tick compares it against
    /// the generation the current `StyleIndex` was built from.
    #[must_use]
    pub fn script_generation(&self) -> u64 {
        self.script_generation
    }

    #[must_use]
    pub fn rule_count(&self, sheet: usize) -> usize {
        self.sheets
            .get(sheet)
            .map_or(0, |sheet| sheet.rules.rules.len())
    }

    /// `CSSRule.cssText` for one rule.
    #[must_use]
    pub fn rule_text(&self, sheet: usize, index: usize) -> Option<String> {
        self.rule_at(sheet, index).map(rule_to_css)
    }

    /// `CSSStyleRule.selectorText`, absent for a rule that carries no selector.
    #[must_use]
    pub fn selector_text(&self, sheet: usize, index: usize) -> Option<String> {
        match self.rule_at(sheet, index)? {
            Rule::Style(rule) => Some(selector_list_to_css(&rule.selectors)),
            Rule::At(_) => None,
        }
    }

    /// `CSSStyleRule.style.cssText`, the rule's declaration block.
    #[must_use]
    pub fn declaration_text(&self, sheet: usize, index: usize) -> Option<String> {
        match self.rule_at(sheet, index)? {
            Rule::Style(rule) => Some(declarations_to_css(&rule.declarations)),
            Rule::At(_) => None,
        }
    }

    fn rule_at(&self, sheet: usize, index: usize) -> Option<&Rule> {
        self.sheets.get(sheet)?.rules.rules.get(index)
    }

    /// Parse `text` as one rule and splice it at `index`, per CSSOM
    /// `insertRule`. Returns the index the rule took.
    pub fn insert_rule(
        &mut self,
        sheet: usize,
        text: &str,
        index: usize,
    ) -> Result<usize, SheetError> {
        let target = self.sheets.get_mut(sheet).ok_or(SheetError::NoSuchSheet)?;
        if index > target.rules.rules.len() {
            return Err(SheetError::IndexSize);
        }
        let parsed = parse_stylesheet(text).map_err(|_| SheetError::Syntax)?;
        let [rule] = <[Rule; 1]>::try_from(parsed.rules).map_err(|_| SheetError::Syntax)?;
        target.rules.rules.insert(index, rule);
        target.scripted = true;
        self.script_generation += 1;
        Ok(index)
    }

    /// Remove the rule at `index`, per CSSOM `deleteRule`.
    pub fn delete_rule(&mut self, sheet: usize, index: usize) -> Result<(), SheetError> {
        let target = self.sheets.get_mut(sheet).ok_or(SheetError::NoSuchSheet)?;
        if index >= target.rules.rules.len() {
            return Err(SheetError::IndexSize);
        }
        target.rules.rules.remove(index);
        target.scripted = true;
        self.script_generation += 1;
        Ok(())
    }

    /// Set `CSSStyleSheet.disabled`, which drops the sheet from the cascade
    /// while it stays in the list script enumerates.
    pub fn set_disabled(&mut self, sheet: usize, disabled: bool) -> Result<(), SheetError> {
        let target = self.sheets.get_mut(sheet).ok_or(SheetError::NoSuchSheet)?;
        if target.disabled != disabled {
            target.disabled = disabled;
            self.script_generation += 1;
        }
        Ok(())
    }

    /// Intern the selectors of every sheet script spliced.
    ///
    /// `parse_stylesheet` carries no interner, so a scripted rule's
    /// `SelectorIdent` holds no atom and matches by string comparison alone.
    /// Running the document's interner over it restores the atom path the
    /// class and id buckets take.
    pub fn intern_scripted(&mut self, interner: &mut SilkInterner) {
        for sheet in &mut self.sheets {
            if sheet.scripted {
                intern_rules(&mut sheet.rules.rules, interner);
                sheet.scripted = false;
            }
        }
    }
}
