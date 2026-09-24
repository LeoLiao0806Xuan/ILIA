# Normalized-text corpus implementation

## Conclusion

The next-release corpus has been derived as 49 normalized UTF-8 text artifacts
under `corpus/normalized/`, with a separate `data/ilia.sqlite3` database. The
50-document baseline and downloaded PDFs remain untouched as provenance inputs,
but are not staged into the installer.

This technical feasibility does not itself create redistribution rights. A
yellow matrix row still needs permission or jurisdiction-specific legal review.

## Implemented changes

1. Add `artifact_format` (`pdf` or `normalized_text`) and a stable
   `source_locator_scheme` to each document version.
2. Define a UTF-8 normalized-text format with immutable structural markers, for
   example document title, part/chapter, article, paragraph and source URL.
3. Replace PDF page as the universal locator. Treat article/paragraph as the
   primary locator for normalized legal text and retain the source-edition page
   only as optional provenance, never as a claim that ILIA redistributes that
   edition.
4. Extend `import_corpus.py` with a text loader. Its output can reuse the current
   `Page`/`Unit` pipeline internally, but database and UI labels must not call
   synthetic text segments "PDF pages".
5. The desktop command is now `read_document_text` and renders the normalized
   text inside ILIA.
6. The distributable database retains the existing chunks and vectors for the
   49 included documents and removes the ICRC customary IHL study. Structural
   validation and a revised retrieval evaluation are release gates.

## Integrity and provenance requirements

Each normalized artifact should have:

- canonical and localized title;
- authentic language and translation status;
- adopting/issuing body and date;
- official source URL and access date;
- normalization procedure and tool version;
- SHA-256 of both the normalized artifact and the source used to verify it;
- an explicit list of removed matter (logos, headers, images, editorial notes,
  publisher pagination); and
- a second-person review status for every changed legal provision.

Normalization must be mechanical: Unicode normalization, whitespace repair,
line-break repair and structural tagging. It must not paraphrase, summarize or
silently correct the legal text.

## Release state

The migration covers all 49 retained documents. `icrc-cihl-rules` is absent
from both `corpus/normalized/` and `data/ilia.sqlite3`. The installer verifies
all artifact sizes and SHA-256 values and rejects any staged corpus PDF.
