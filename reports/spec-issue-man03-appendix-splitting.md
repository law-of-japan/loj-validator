# Clarify MAN-03 appendix file starts and Markdown block completion at EOF

## Specification and existing issue search

- loj/v1alpha1, commit `b2356c6f113f3a02a1a790dc5cf3813d82cf3fe0`.
- MAN-03, MD-01/03, HEAD-03; both normative EBNF grammars; fixed conformance index.
- Searched all open and closed issues in law-of-japan/loj-spec on 2026-11-06 JST.
  Only #1 (L0 positives and diagnostic positions) existed; it does not address this question.

## Questions requiring a normative decision

MAN-03 permits splitting appendices between completed Markdown blocks, but its next
sentence requires subsequent files to start with an article or classification heading.
HEAD-03 makes appendix content opaque and treats subsequent headings as ordinary text.
Does the article/classification start condition exclude appendix scopes? If so, what
exactly may start subsequent appendix files, including blank lines and ordinary text?

MAN-03 prohibits continuing Markdown blocks across files and allows only completed
blocks at a split. MD-03 explicitly notes that CommonMark treats an unclosed fenced
code block as code through EOF. Does EOF completion satisfy MAN-03, or must a fence
be explicitly closed before a non-final appendix file ends? Is the final file different?
Related HTML blocks can also terminate at EOF; the general completion criterion matters.

## Minimal inputs

Use an otherwise valid main.md and a declared appendix with key `table`, title `別表`.
List main.md, one.md and two.md in that order, assigning the last two to appendix/table.

Ordinary block split:

    one.md: "# 別表\nfirst paragraph\n\n"
    two.md: "second paragraph\n"

Fence split (the fences are literal input bytes):

    one.md: "# 別表\n```\ncode\n"
    two.md: "continued\n```\n"

Each file can be parsed independently, but the second example renders differently
when concatenated. Neither EOF termination nor author intent alone establishes the
intended legal partition contract.

## Normative grammar and fixture comparison

headings.ebnf refers appendix headings to HEAD-03; directives.ebnf defines marker
tokens only. Neither defines a cross-file Markdown completion predicate.
The fixed appendix positive T052 uses one appendix file and therefore cannot decide
either question. The MAN-03 cases cover scope ordering, structural continuation and
other file constraints, but no fixed case decides multi-file appendix starts or
explicit fence closure at an appendix split.

## Requested resolution

Please specify the scope of the subsequent-file heading requirement and the exact
block-completion condition, including fenced code and HTML blocks at EOF. Add
independently authored positive/negative cases and, where applicable, the required
diagnostic rule and byte location. This request does not prescribe one interpretation.

## Validator handling while unresolved

loj-validator previously accepted these multi-file appendix inputs. It now reports
L1 Incomplete (exit 3), with code `loj/v1alpha1/specification-pending` and rule MAN-03,
and does not expose a complete AST. L0 input checks remain available. The pending
status is an implementation limitation while awaiting clarification, not a claim
that all split appendices violate the specification. Single-file appendix behavior
is unchanged. No specification, schema or official fixture was modified.

The 74 formal L0/L1 cases contain no affected multi-file appendix and remain separately
reportable; local pending-contract tests must not be counted as resolved conformance.
