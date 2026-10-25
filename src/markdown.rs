use markraft_commonmark::{CommonMarkCodecs, CommonMarkSpelling, Formatter, HouseStyleHandle};
use markraft_core::{
    Attrs, MarkTypeId, Schema,
    commands::Command,
    kind::{Codecs, DocumentKind, Formatting, SourceSpelling},
};
use std::sync::Arc;

pub struct Markdown {
    pub schema: Schema,
}
impl Markdown {
    fn formatter(&self) -> Formatter {
        Formatter::new(HouseStyleHandle::default())
    }
}
fn worded(command: markraft_commonmark::FormatCommand) -> Formatting {
    Arc::new(move |state| {
        command(state).map_err(|error| {
            log::warn!("Formatting has no Markdown form: {error}");
            "This formatting can’t be written as Markdown here.".to_owned()
        })
    })
}
impl DocumentKind for Markdown {
    fn codecs(&self) -> Option<Arc<dyn Codecs>> {
        Some(Arc::new(CommonMarkCodecs::new(
            self.schema.clone(),
            HouseStyleHandle::default(),
        )))
    }
    fn spelling(&self) -> Option<Arc<dyn SourceSpelling>> {
        Some(Arc::new(CommonMarkSpelling::new(self.schema.clone())))
    }
    fn toggle_mark(&self, ty: MarkTypeId, _: Attrs) -> Option<Formatting> {
        Some(worded(self.formatter().toggle_style(ty)))
    }
    fn set_link(&self, _: MarkTypeId, url: Option<&str>) -> Option<Formatting> {
        Some(worded(match url {
            Some(url) => self.formatter().set_link(url, ""),
            None => self.formatter().unlink(),
        }))
    }
    fn replace_reading(
        &self,
        range: std::ops::Range<usize>,
        text: &str,
        policy: markraft_core::kind::ReadingReplacementPolicy,
    ) -> Option<Formatting> {
        Some(worded(
            self.formatter().replace_reading(range, text, policy),
        ))
    }
    fn wrap_split(&self, split: Command) -> Command {
        self.formatter().keeping_styles(split)
    }
    fn enter_rule(&self) -> Option<Command> {
        Some(markraft_commonmark::block_from_line())
    }
}
