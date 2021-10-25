use crate::{pcre, siteinfo};
use err_derive::Error;
use pcre::HirExt;
use regex_syntax::hir;
use std::{collections, iter};

#[derive(Debug, Error)]
#[error(display = "namespace not found: {:?}", _0)]
pub(crate) struct NamespaceNotFoundError(String);

#[derive(Debug, Error)]
#[error(display = "malformed extension tag: {:?}", _0)]
pub(crate) struct MalformedExtensionTagError(String);

#[derive(Debug, Error)]
pub(crate) enum LinkTrailError {
    #[error(display = "{}", _0)]
    PCRE(#[error(source)] pcre::PatternParseError),
    #[error(
        display = "group {} not found in link trail pattern: {:?}",
        index,
        pattern
    )]
    GroupNotFound { pattern: String, index: u32 },
    #[error(
        display = "group {} of invalid structure in link trail pattern: {:?}",
        index,
        pattern
    )]
    GroupInvalid { pattern: String, index: u32 },
}

pub(crate) fn namespaces(
    query: &siteinfo::response::Query,
    canonical: &str,
) -> Result<collections::BTreeSet<String>, NamespaceNotFoundError> {
    let namespace = query
        .namespaces
        .values()
        .find(|ns| ns.canonical.as_ref().map(AsRef::as_ref) == Some(canonical))
        .ok_or_else(|| NamespaceNotFoundError(canonical.to_owned()))?;
    let aliases = query
        .namespacealiases
        .iter()
        .filter(|na| na.id == namespace.id);
    let names = aliases
        .map(|na| na.alias.as_str())
        .chain(iter::once(canonical))
        .chain(iter::once(namespace.name.as_str()))
        .map(str::to_lowercase);
    Ok(names.collect())
}

pub(crate) fn extension_tags(
    query: &siteinfo::response::Query,
) -> Result<collections::BTreeSet<String>, MalformedExtensionTagError> {
    query
        .extensiontags
        .iter()
        .map(|et| {
            et.0.as_str()
                .strip_prefix("<")
                .and_then(|s| s.strip_suffix(">"))
                .map(str::to_lowercase)
                .ok_or(MalformedExtensionTagError(et.0.clone()))
        })
        .collect()
}

pub(crate) fn protocols(query: &siteinfo::response::Query) -> collections::BTreeSet<String> {
    query.protocols.iter().map(|p| p.0.to_lowercase()).collect()
}

pub(crate) fn link_trail(
    query: &siteinfo::response::Query,
) -> Result<collections::BTreeSet<char>, LinkTrailError> {
    use hir::HirKind::*;

    let original = &query.general.linktrail;
    let pattern: pcre::Pattern = original.parse().map_err(LinkTrailError::PCRE)?;
    log::debug!("pattern = {:?}", pattern);

    const GROUP_INDEX: u32 = 1;
    let group =
        pattern
            .hir
            .find_group_index(GROUP_INDEX)
            .ok_or_else(|| LinkTrailError::GroupNotFound {
                pattern: original.clone(),
                index: GROUP_INDEX,
            })?;
    let repeated = match group.hir.kind() {
        Empty => Ok(None),
        Repetition(repetition) => Ok(Some(&repetition.hir)),
        Alternation(..) | Anchor(..) | Class(..) | Concat(..) | Group(..) | Literal(..)
        | WordBoundary(..) => Err(LinkTrailError::GroupInvalid {
            pattern: original.clone(),
            index: GROUP_INDEX,
        }),
    }?;
    log::debug!("repeated = {:?}", repeated.map(|r| pcre::HirDebugAlt(r)));

    let mut characters = Default::default();
    if let Some(repeated) = repeated {
        link_trail_characters(repeated, &mut characters).map_err(|_| {
            LinkTrailError::GroupInvalid {
                pattern: original.clone(),
                index: GROUP_INDEX,
            }
        })?;
    }
    Ok(characters)
}

fn link_trail_characters(
    hir: &hir::Hir,
    characters: &mut collections::BTreeSet<char>,
) -> Result<(), ()> {
    use hir::HirKind::*;
    use hir::{Class, Literal};
    match hir.kind() {
        Alternation(hirs) => {
            for hir in hirs {
                link_trail_characters(hir, characters)?;
            }
            Ok(())
        }
        Class(class) => {
            match class {
                Class::Bytes(bytes) => {
                    for range in bytes.iter() {
                        for b in range.start()..=range.end() {
                            debug_assert!(b.is_ascii());
                            characters.insert(b.into());
                        }
                    }
                }
                Class::Unicode(unicode) => {
                    for range in unicode.iter() {
                        for c in range.start()..=range.end() {
                            characters.insert(c);
                        }
                    }
                }
            }
            Ok(())
        }
        Group(group) => link_trail_characters(&group.hir, characters),
        Literal(literal) => {
            let c = match literal {
                Literal::Byte(..) => unreachable!(),
                Literal::Unicode(c) => *c,
            };
            characters.insert(c);
            Ok(())
        }
        Anchor(..) | Concat(..) | Empty | Repetition(..) | WordBoundary(..) => Err(()),
    }
}

pub(crate) fn magic_words(query: &siteinfo::response::Query) -> collections::BTreeSet<String> {
    query
        .magicwords
        .iter()
        .flat_map(|mw| {
            mw.aliases
                .iter()
                .map(AsRef::as_ref)
                .chain(iter::once(mw.name.as_str()))
        })
        .filter_map(|s| s.strip_prefix("__").and_then(|s| s.strip_suffix("__")))
        .map(str::to_lowercase)
        .collect()
}

pub(crate) fn magic_words_redirect(
    query: &siteinfo::response::Query,
) -> collections::BTreeSet<String> {
    const NAME: &str = "redirect";
    const PREFIX: &str = "#";
    query
        .magicwords
        .iter()
        .filter(|mw| mw.name == NAME)
        .flat_map(|mw| mw.aliases.iter())
        .map(|s| s.strip_prefix(PREFIX).unwrap_or(s))
        .chain(iter::once(NAME))
        .map(str::to_lowercase)
        .collect()
}
