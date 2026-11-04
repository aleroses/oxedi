# Writing 835 files

`oxedi.write` turns tables into an 835 file. The tables can come from a parse, or be your own
Arrow, Polars or pandas tables with the spec's columns.

To reproduce a parsed file byte for byte, you do not need the writer: use
`result.document.write()` (see [Reading](reading.md#get-the-file-back-byte-for-byte)).

## Parse, change and write a file

This example changes the trace number of the example file and writes a new file:

```python
import datetime
import polars as pl
import oxedi

result = oxedi.parse_file("remittance.835")
frames = result.tables.to_polars()   # {table name: polars frame}
frames["payments"] = frames["payments"].with_columns(trace_number=pl.lit("CHK100235"))

envelope = oxedi.Envelope(
    sender_id="ACMEPAYER",
    receiver_id="SUNRISECLINIC",
    date=datetime.date(2024, 1, 10),
    time=datetime.time(9, 0),
    usage_indicator="T",  # "P" (production) is the default
    line_break=True,
)
data = oxedi.write(frames, envelope, spec=result.spec)  # bytes, one interchange
print("\n".join(data.decode().splitlines()[:5]))  # the first five segments
```

```text
ISA*00*          *00*          *ZZ*ACMEPAYER      *ZZ*SUNRISECLINIC  *240110*0900*^*00501*000000001*0*T*:~
GS*HP*ACMEPAYER*SUNRISECLINIC*20240110*0900*1*X*005010X221A1~
ST*835*0001~
BPR*I*690*C*CHK************20240112~
TRN*1*CHK100235*1512345678~
```

- `write` returns the file as bytes. Save it with `Path("out.835").write_bytes(data)`.
- The writer is strict. If the money does not balance or the file would not read back clean,
  it raises `oxedi.WriteError` and writes nothing. See
  [When the writer refuses a file](when-write-refuses.md).

## Pass the right spec

The writer uses the spec that parsed the tables, else the built-in 5010 spec.

- A dictionary of frames, as above, does not carry the parse's spec. Pass it:
  `spec=result.spec` (`result.tables.spec` is the same one).
- For a 4010 file you can also pass `oxedi.Spec.builtin(oxedi.Version.V4010)`.

!!! warning "4010 files"
    Without the 4010 spec, a 4010 file is written as 5010 and refused.

## Know what is written

- A table you leave out has no rows. A column you leave out is null.
- Rows nest by their `payment`, `claim` and `service` columns, the row number of their parent.
- Only what the tables hold is written. Segments no column reads are left out, for example the
  payer's `PER*CX` or the bank details of `BPR`.

??? note "Details: the transaction header"
    The transaction set header carries no implementation convention reference, so a written
    5010 header is `ST*835*0001~`.

<a id="envelope"></a>

## Fill in the envelope

`oxedi.Envelope` gives what the tables cannot:

- `sender_id` and `receiver_id`, with their qualifiers `sender_qualifier` and
  `receiver_qualifier` (default `"ZZ"`).
- `date` and `time`.
- `usage_indicator` (`"P"` by default).
- `control_number`, the first one (default 1).
- `application_sender` and `application_receiver`, optional, for the group header.
- `delimiters`: default `*`, `:`, `~` and repetition `^`.
- `line_break`: a line break after each segment.

The writer works out the rest:

- The fixed-width `ISA`.
- The control numbers that must match: `ISA13`/`IEA02`, `GS06`/`GE02`, `ST02`/`SE02`.
- The counts: `SE01`, `GE01`, `IEA01`.

## Write from SQL

The DuckDB extension writes the same file from SQL with `COPY`. For the same tables and
envelope, the bytes equal those of `oxedi.write`. See [Writing with COPY](../duckdb/writing.md).

## Next

- [Writing from your own tables](own-tables.md){ #from-your-own-tables }: number your rows the way the writer expects.
- [When the writer refuses a file](when-write-refuses.md){ #findings }: findings, money and empty text.
