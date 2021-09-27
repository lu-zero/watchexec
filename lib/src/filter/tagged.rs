use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use globset::GlobMatcher;
use regex::Regex;
use tracing::warn;

use crate::error::RuntimeError;
use crate::event::{Event, Tag};
use crate::filter::Filterer;

mod parse;

pub struct TaggedFilterer {
	/// The directory the project is in, its "root".
	///
	/// This is used to resolve absolute paths without an `in_path` context.
	_root: PathBuf,

	/// Where the program is running from.
	///
	/// This is used to resolve relative paths without an `in_path` context.
	_workdir: PathBuf,

	/// All filters that are applied, in order, by matcher.
	filters: HashMap<Matcher, Vec<Filter>>,
}

impl Filterer for TaggedFilterer {
	fn check_event(&self, event: &Event) -> Result<bool, RuntimeError> { // TODO: trace logging
		if self.filters.is_empty() {
			return Ok(true);
		}

		for tag in &event.tags {
			if let Some(tag_filters) = self.filters.get(&tag.into()) {
				if tag_filters.is_empty() {
					continue;
				}

				let mut tag_match = true;
				for filter in tag_filters {
					if let Some(app) = self.match_tag(filter, tag)? {
						if filter.negate {
							if app {
								tag_match = true;
							}
						} else {
							tag_match &= app;
						}
					}
				}

				if !tag_match {
					return Ok(false);
				}
			}
		}

		Ok(true)
	}
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filter {
	/// Path the filter applies from.
	pub in_path: Option<PathBuf>,

	/// Which tag the filter applies to.
	pub on: Matcher,

	/// The operation to perform on the tag's value.
	pub op: Op,

	/// The pattern to match against the tag's value.
	pub pat: Pattern,

	/// If true, a positive match with this filter will override negative matches from previous
	/// filters on the same tag, and negative matches will be ignored.
	pub negate: bool,
}

impl TaggedFilterer {
	fn match_tag(&self, filter: &Filter, tag: &Tag) -> Result<Option<bool>, RuntimeError> {
		match (tag, filter.on) {
			(tag, Matcher::Tag) => filter.matches(tag.discriminant_name()),
			(Tag::Path(_path), Matcher::Path) => todo!("tagged filterer: path matcher"),
			(Tag::FileEventKind(kind), Matcher::FileEventKind) => filter.matches(format!("{:?}", kind)),
			(Tag::Source(src), Matcher::Source) => filter.matches(src.to_string()),
			(Tag::Process(pid), Matcher::Process) => filter.matches(pid.to_string()),
			(Tag::Signal(_sig), Matcher::Signal) => todo!("tagged filterer: signal matcher"),
			(Tag::ProcessCompletion(_oes), Matcher::ProcessCompletion) => todo!("tagged filterer: completion matcher"),
			_ => return Ok(None),
		}.map(Some)
	}
}

impl Filter {
	pub fn matches(&self, subject: impl AsRef<str>) -> Result<bool, RuntimeError> {
		let subject = subject.as_ref();

		// TODO: cache compiled globs

		match (self.op, &self.pat) {
			(Op::Equal, Pattern::Exact(pat)) => Ok(subject == pat),
			(Op::NotEqual, Pattern::Exact(pat)) => Ok(subject != pat),
			(Op::Regex, Pattern::Regex(pat)) => Ok(pat.is_match(subject)),
			(Op::NotRegex, Pattern::Regex(pat)) => Ok(!pat.is_match(subject)),
			(Op::Glob, Pattern::Glob(pat)) => Ok(pat.is_match(subject)),
			(Op::NotGlob, Pattern::Glob(pat)) => Ok(!pat.is_match(subject)),
			(Op::InSet, Pattern::Set(set)) => Ok(set.contains(subject)),
			(Op::InSet, Pattern::Exact(pat)) => Ok(subject == pat),
			(Op::NotInSet, Pattern::Set(set)) => Ok(!set.contains(subject)),
			(Op::NotInSet, Pattern::Exact(pat)) => Ok(subject != pat),
			(op, pat) => {
				warn!("trying to match pattern {:?} with op {:?}, that cannot work", pat, op);
				Ok(false)
			}
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[non_exhaustive]
pub enum Matcher {
	Tag,
	Path,
	FileEventKind,
	Source,
	Process,
	Signal,
	ProcessCompletion,
}

impl From<&Tag> for Matcher {
	fn from(tag: &Tag) -> Self {
		match tag {
			Tag::Path(_) => Matcher::Path,
			Tag::FileEventKind(_) => Matcher::FileEventKind,
			Tag::Source(_) => Matcher::Source,
			Tag::Process(_) => Matcher::Process,
			Tag::Signal(_) => Matcher::Signal,
			Tag::ProcessCompletion(_) => Matcher::ProcessCompletion,
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Op {
	Auto,     // =
	Equal,    // ==
	NotEqual, // !=
	Regex,    // ~=
	NotRegex, // ~!
	Glob,     // *=
	NotGlob,  // *!
	InSet,    // :=
	NotInSet, // :!
}

#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Pattern {
	Exact(String),
	Regex(Regex),
	Glob(GlobMatcher),
	Set(HashSet<String>),
}

impl PartialEq<Self> for Pattern {
	fn eq(&self, other: &Self) -> bool {
		match (self, other) {
			(Self::Exact(l), Self::Exact(r)) => l == r,
			(Self::Regex(l), Self::Regex(r)) => l.as_str() == r.as_str(),
			(Self::Glob(l), Self::Glob(r)) => l.glob() == r.glob(),
			(Self::Set(l), Self::Set(r)) => l == r,
			_ => false,
		}
	}
}

impl Eq for Pattern {}