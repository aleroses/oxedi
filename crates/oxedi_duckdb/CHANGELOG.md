# Changelog

All notable changes to the oxedi DuckDB extension are documented here. This file covers the
extension only; the Python package's changes are in [`CHANGELOG.md`](../../CHANGELOG.md) at the
repository root. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)
and the extension has its own version.

## [Unreleased]

### Added
- `read_835(..., mask_key := ..., mask_dates := 'year' | 'shift' | 'keep')` hides patient data:
  the columns the spec flags are replaced by keyed 16-character tokens or coarser dates, and the
  diagnostics are masked the same way. A NULL or short key is refused, and `mask_dates` without
  `mask_key` is an error.
- The table function `oxedi_schema(version := '5010', binary := false)`: every table `read_835`
  returns (`diagnostics` included), one row per column with `table_name`, `column_name`,
  `column_type` (the type `DESCRIBE` gives for `read_835` with the same `version` and `binary`),
  `parent` (the table a reference column points at, else NULL) and `position`, without a file.

### Changed
- Without `table_name`, `read_835` returns the table the chosen spec declares as its
  `default_table` (`claims` for the built-in 835, as before).
- `read_835` fills only the table it reads instead of every table of the spec and then keeping
  one, so each scan of one table takes about a fifth less time (900 files, 127 MB, one thread:
  `table_name := 'claims'` from 4.6 s to 3.6 s). The rows are unchanged, and the `diagnostics`
  table is unchanged.

### Fixed
- `COPY ... (FORMAT edi835)` accepts a zero decimal that only scales up and names an
  overflowing scale as an overflow.
- The macro-parameter error of `read_835` and `oxedi_schema` names the parameter only when
  DuckDB's qualified name has a single dot; with more dots the macro's name may contain a dot too,
  so the error says `write table_name = <the parameter>` instead of guessing.
- A table macro that forwards its parameter to `table_name` or `version` with `:=` while the
  path is fixed now fails with an error that says the value must be a constant string and to write
  the argument with `=` (`table_name = tbl`), which forwards the parameter, instead of naming
  DuckDB's internal identifier as an unknown table or version.

## [0.2.0] - 2026-10-12

### Added
- The copy format `edi835`: `COPY (SELECT {'payments': ..., 'claims': ...}) TO 'out.835' (FORMAT
  edi835, sender_id ..., receiver_id ..., date ..., time ...)` writes the tables of the spec
  back into an 835 with the core writer, byte for byte equal to `oxedi.write` with the same
  envelope. Its options are those of `oxedi.Envelope` and `version`. It is strict: any finding
  fails the `COPY` with the listing `oxedi.WriteError` shows and nothing is written. The `time`
  option takes a `TIME`, a `TIME_NS` or text, and a time column takes `TIME` or `TIME_NS` in
  whole seconds.
  The transaction set header is written without its implementation convention reference
  (`ST03`), as `oxedi.write` writes it.

## [0.1.2] - 2026-10-06

### Fixed
- A test compared the file names a glob returns with forward slashes; on Windows the operating
  system writes them with backslashes. The test now normalizes the separators, so the community
  build passes on Windows. 0.1.1 was never published because of it.

## [0.1.1] - 2026-10-06

### Fixed
- The test of a file that cannot be opened no longer pins the operating system's error text, so
  the extension's tests pass on Windows and DuckDB's community build publishes it on every
  platform. 0.1.0 was never published because that test failed on Windows.

### Added
- The diagnostics table carries six new level-2 rules from the loop structure:
  `RequiredOccurrenceMissing`, `OccurrenceOverMax`, `LoopOverMax`, `OutOfOrder`,
  `UnknownOccurrence` and `RequiredLoopMissing`; `CodeNotInList` may check an element against
  the code list of the occurrence its segment takes.
- The diagnostics table carries the level-3 rule `BalanceMismatch`: a balancing rule of the spec
  (service, claim and transaction balancing in the built-in 835) that does not add up.

### Changed
- A text cell whose element is present but empty is now `''`; an absent element stays `NULL`
  (both used to be `NULL`). Numeric and date columns keep `NULL` for both.

## [0.1.0] - 2026-10-05

### Added
- The DuckDB extension `oxedi`, installed with `INSTALL oxedi FROM community; LOAD oxedi;`:
  `read_835(...)` returns the payments, claims, services, adjustments, provider adjustments and
  diagnostics tables in SQL.

[Unreleased]: https://github.com/javillegasna/oxedi/compare/duckdb-v0.2.0...HEAD
[0.2.0]: https://github.com/javillegasna/oxedi/releases/tag/duckdb-v0.2.0
[0.1.2]: https://github.com/javillegasna/oxedi/releases/tag/duckdb-v0.1.2
[0.1.1]: https://github.com/javillegasna/oxedi/releases/tag/duckdb-v0.1.1
[0.1.0]: https://github.com/javillegasna/oxedi/releases/tag/duckdb-v0.1.0
