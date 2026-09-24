# V1 corpus redistribution review

Review date: 2026-09-24

This is a conservative release-risk review, not legal advice and not a
jurisdiction-specific conclusion about whether a legal text is copyrightable.
The controlling row-by-row record is
`licenses/corpus-redistribution-rights-matrix.csv`.

## Rating rule

- **Green**: an express unrestricted redistribution grant or written permission
  supports the exact artifact being shipped. None of the current 50 source PDFs
  qualifies.
- **Yellow**: the underlying legal or judicial text may be usable separately,
  but the project must remove publisher layout, logos, images, editorial
  material and other edition-specific elements, preserve provenance, and obtain
  permission or jurisdiction-specific legal review before release.
- **Red**: do not put the artifact or text in a public installer without express
  permission. Metadata and an official source link may remain.

Ratings are recorded separately for normalized text and the downloaded source
PDF. A public download is not treated as a redistribution licence.

## Findings

1. The United Nations Treaty Collection usage agreement permits personal,
   non-commercial downloading and copying but expressly withholds resale and
   redistribution rights. The general UN terms likewise do not provide a broad
   redistribution licence for these materials.
2. The ICRC copyright terms prohibit commercial use or publication without
   prior express authorization. For personal/non-commercial copying they permit
   documents to be copied only with copyright and source indications, complete
   and unmodified. Those PDFs are therefore yellow rather than red for ILIA's
   current free, non-commercial release, but the restriction must remain
   separate from the Apache-2.0 code licence.
3. No express worldwide redistribution licence was located for the ICJ PDF
   fascicles. Their status as official judicial texts may matter under a
   particular country's law, but that is not a safe basis for worldwide binary
   distribution without a targeted legal opinion or Registry permission.
4. The ICRC customary IHL rules are an authored study/publication, not merely a
   treaty text. Its unchanged PDF is yellow only under the ICRC non-commercial
   copying conditions; its normalized full text remains red.

## Release decision

Do not rebuild or publish v1.0.1 with the current 50 source PDFs unchanged as a
single Apache-2.0 payload. The implemented replacement keeps those provenance
inputs untouched in the working tree but excludes them from installer staging.

The next-release implementation:

1. independently produces 49 normalized legal/judicial text artifacts with no
   logos, scans, publisher pagination or editorial content;
2. preserves title, authentic-language status, source URL, retrieval date and
   content hash in a new artifact manifest;
3. excludes the ICRC customary IHL rules study entirely from the release
   database, normalized artifacts and vectors;
4. presents a separate corpus notice during installation and in the installed
   files so Apache-2.0 is not represented as covering third-party documents.

Written permission or a jurisdiction-specific legal review remains the safest
way to upgrade yellow rows to green. The matrix deliberately records the 49
normalized artifacts as yellow rather than claiming universal clearance.

Official policy references:

- <https://www.un.org/en/about-us/terms-of-use>
- <https://treaties.un.org/pages/Overview.aspx?path=overview/usageAgreement/page1_en.xml>
- <https://www.icrc.org/en/copyright-and-terms-use>
- <https://www.icj-cij.org/contact-the-court>

The matrix can be regenerated with:

```powershell
python tools/generate-corpus-rights-matrix.py
```
