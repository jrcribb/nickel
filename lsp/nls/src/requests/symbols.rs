use lsp_server::{RequestId, Response, ResponseError};
use lsp_types::{DocumentSymbol, DocumentSymbolParams, SymbolKind};
use nickel_lang_core::term::RichTerm;
use nickel_lang_core::typ::Type;

use crate::analysis::CollectedTypes;
use crate::cache::CacheExt as _;
use crate::field_walker::{FieldResolver, Record};
use crate::server::Server;
use crate::term::RawSpanExt;
use crate::world::World;

// Returns a hierarchy of "publicly accessible" symbols in a term.
//
// Basically, if the term "evaluates" (in the sense of FieldResolver's heuristics) to a record,
// all fields in that record count as publicly accessible symbols. Then we recurse into
// each of those.
fn symbols(
    world: &World,
    type_lookups: &CollectedTypes<Type>,
    rt: &RichTerm,
) -> Vec<DocumentSymbol> {
    let resolver = FieldResolver::new(world);
    let root_records = resolver.resolve_path(rt, [].into_iter());
    root_records
        .into_iter()
        .filter_map(|rec| match rec {
            Record::RecordTerm(data) => Some(data),
            Record::RecordType(_) => None,
        })
        .flat_map(|rt| {
            rt.fields.into_iter().filter_map(|(id, field)| {
                let ty = type_lookups.idents.get(&id.into());
                let (file_id, span) = id.pos.into_opt()?.to_range();
                let range =
                    crate::codespan_lsp::byte_span_to_range(world.cache.files(), file_id, span)
                        .ok()?;

                let children = field.value.map(|v| symbols(world, type_lookups, &v));

                #[allow(deprecated)]
                // because the `deprecated` field is... wait for it... deprecated.
                Some(DocumentSymbol {
                    name: id.ident().to_string(),
                    detail: ty.map(Type::to_string),
                    kind: SymbolKind::VARIABLE,
                    tags: None,
                    range,
                    selection_range: range,
                    children,
                    deprecated: None,
                })
            })
        })
        .collect()
}

pub fn handle_document_symbols(
    params: DocumentSymbolParams,
    id: RequestId,
    server: &mut Server,
) -> Result<(), ResponseError> {
    let file_id = server
        .world
        .cache
        .file_id(&params.text_document.uri)?
        .ok_or_else(|| crate::error::Error::FileNotFound(params.text_document.uri.clone()))?;

    let type_lookups = &server.world.file_analysis(file_id)?.type_lookup;
    let term = server.world.cache.get_ref(file_id);

    let mut symbols = term
        .map(|t| symbols(&server.world, type_lookups, t))
        .unwrap_or_default();
    // Sort so the response is deterministic.
    symbols.sort_by_key(|s| s.range.start);

    server.reply(Response::new_ok(id, symbols));

    Ok(())
}
