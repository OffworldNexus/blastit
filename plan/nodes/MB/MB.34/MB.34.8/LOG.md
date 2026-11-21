# Log MB.34.8

## 2026-11-18T01:26 [lead]

created (task) State every query refusal and quote the two hover paragraphs on the pages (design MB.34.2 supersedes the page's silence)

## 2026-11-18T01:26 [lead]

Scope widened by MB.34.2's design: the three refusal rows are Syntax, TooLong (65_536 chars) and TooManyQueries (4_096 entries), plus two clauses (a `query "…"` string is not SQL-coloured; the Quick Fix also fires on a $1…$n paste). Design §13.4/§7.6/§7A is authoritative over this brief.

## 2026-11-18T01:26 [lead]

status ready -> active

## 2026-11-18T01:26 [lead]

attempt 1/3 started

## 2026-11-18T01:26 [lead]

base 5f293985cc355b29dbac08421482b8ea84d1c677

## 2026-11-18T01:37 [lead]

status active -> review

## 2026-11-18T01:37 [lead]

owner done; dx critic CHANGES->PASS; reviewer dispatched

## 2026-11-18T01:48 [lead]

reviewer VERDICT PASS; 3 reviewer MINORs applied (StrayEllipsis span renders whole — verified in site HTML, ## Errors claim narrowed, 'can quote'). Committing with MB.34.1.

## 2026-11-18T05:02 [lead]

committed d174253

## 2026-11-18T05:02 [lead]

status review -> done
