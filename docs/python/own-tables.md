# Writing from your own tables

Your tables do not have to come from a parse. This page shows how to shape them so
`oxedi.write` or `COPY` accepts them. Read [Writing 835 files](writing.md) first for the
envelope.

## What the writer accepts

`oxedi.write` and `COPY` take the spec's table and column names and nothing else.

Tolerated:

- A missing table. It has no rows.
- A missing column. It is null.
- A column of a narrower or compatible type: an integer for a big integer, or a decimal of
  another scale that holds the value.
- In SQL, `INTEGER` for a `BIGINT` column, and `ENUM` or `BLOB` for a text column.

Refused, naming the table, row and column:

- A table or column that is not in the spec.
- Floats for money.
- A value that would change in the conversion.
- A file whose totals do not balance or that lacks a required element.

## Number your rows

The spec's tables key their rows by position. If your tables have their own keys, translate
them:

1. Number the rows of each table from 0 in a `row` column.
2. Bring the parents' `row` in with a join, into `payment`, `claim` and `service`.
3. Drop your own key columns.

Order matters:

- Rows of one parent must be together and in their parents' order.
- Sort each child table by the parent's `row` first, then by its own key. Keep the table's
  order for ties.
- The adjustments of a claim (no `service`) come before those of its services.

!!! note "Provider adjustments and `PLB`"
    Keep the `segment` column of `provider_adjustments` when you have one. Adjustments with the
    same `segment` are written in one `PLB`. Without it, all the adjustments of a payment share
    one `PLB`.

## See the columns a table needs

Ask the spec. No file is needed, and a patched spec describes its own columns.
[Tables and columns](../reference/tables.md) lists the built-in ones.

=== "Python"

    `Spec.schema()` maps each table name to a `pyarrow.Schema`, with the Arrow types `parse`
    gives that table. A reference column (`payment`, `claim`, `service`) holds the `row` of
    another table, and its field metadata names that table under `oxedi.parent`. It needs
    pyarrow.

    ```python
    import oxedi

    schemas = oxedi.Spec.builtin().schema()
    claims = schemas["claims"]
    print(claims.field("charge_amount").type)
    print(claims.field("payment").metadata)
    ```

    ```text
    decimal128(38, 2)
    {b'oxedi.parent': b'payments'}
    ```

    Pass the spec you write with, such as `oxedi.Spec.builtin(oxedi.Version.V4010)` or a patched one.

=== "SQL"

    `oxedi_schema()` returns one row per column, with the type `read_835` gives it. `parent`
    names the table a reference column points at.

    ```sql
    SELECT column_name, column_type, parent
    FROM oxedi_schema(version := '5010')
    WHERE table_name = 'claims' AND position <= 4;
    ```

    ```text
    ┌───────────────┬───────────────┬──────────┐
    │  column_name  │  column_type  │  parent  │
    │    varchar    │    varchar    │ varchar  │
    ├───────────────┼───────────────┼──────────┤
    │ row           │ BIGINT        │ NULL     │
    │ segment       │ BIGINT        │ NULL     │
    │ payment       │ BIGINT        │ payments │
    │ charge_amount │ DECIMAL(38,2) │ NULL     │
    └───────────────┴───────────────┴──────────┘
    ```

    It also lists the `diagnostics` table, which `read_835` can return but `COPY` does not
    write. See [Reading with SQL](../duckdb/reading.md#see-the-columns-without-a-file).

Neither says which columns a valid file requires: the writer reports a missing required element
by table, row and column.

## Translate your keys

The examples take five tables of your own:

- `my_payments`, keyed by `pay_key`.
- `my_claims`, keyed by `claim_key`, with its `pay_key`.
- `my_services`, keyed by `service_key`, with its `claim_key`.
- `my_adjustments`, with its `claim_key`, and `service_key` when the adjustment is on a service.
- `my_provider_adjustments`, with its `pay_key`.

Each also has the spec's other columns.

=== "Polars"

    Use `with_row_index("row")` to number the rows.

    The examples need `my_*` tables. To try them, build them from the tables of a parse, with
    keys of your own in place of the positions:

    ```python
    import datetime

    import oxedi
    import polars as pl

    frames = oxedi.parse_file("remittance.835").tables.to_polars()
    envelope = oxedi.Envelope(
        sender_id="ACMEPAYER",
        receiver_id="SUNRISECLINIC",
        date=datetime.date(2024, 1, 10),
        time=datetime.time(9, 0),
    )

    def keyed(name, **keys):
        """The spec's table without its positions, keyed as your own system would."""
        table = frames[name].with_columns(
            **{key: pl.col(source) + 1 for key, source in keys.items()}
        )
        return table.drop("row", "payment", "claim", "service", "segment", strict=False)

    my_payments = keyed("payments", pay_key="row")
    my_claims = keyed("claims", claim_key="row", pay_key="payment")
    my_services = keyed("services", service_key="row", claim_key="claim")
    my_adjustments = keyed("adjustments", claim_key="claim").with_columns(
        service_key=frames["adjustments"]["service"] + 1
    )
    my_provider_adjustments = keyed("provider_adjustments", pay_key="payment")
    ```

    ```python
    import polars as pl

    # my_payments (pay_key), my_claims (claim_key, pay_key), my_services (service_key,
    # claim_key), my_adjustments (claim_key, service_key or null), my_provider_adjustments
    # (pay_key): your tables, each with the spec's other columns
    payments = my_payments.sort("pay_key").with_row_index("row")
    claims = (
        my_claims.join(payments.select("row", "pay_key"), on="pay_key", maintain_order="left")
        .rename({"row": "payment"})
        .sort("payment", "claim_key", maintain_order=True)
        .with_row_index("row")
    )
    claim_rows = claims.select(claim="row", payment="payment", claim_key="claim_key")
    services = (
        my_services.join(claim_rows, on="claim_key", maintain_order="left")
        .sort("claim", "service_key", maintain_order=True)
        .with_row_index("row")
    )
    adjustments = (
        my_adjustments.join(claim_rows, on="claim_key", maintain_order="left")
        .join(
            services.select(service="row", service_key="service_key"),
            on="service_key", how="left", maintain_order="left",
        )
        .sort("claim", "service", nulls_last=False, maintain_order=True)
        .with_row_index("row")
    )
    provider_adjustments = (
        my_provider_adjustments.join(
            payments.select(payment="row", pay_key="pay_key"), on="pay_key", maintain_order="left"
        )
        .sort("payment", maintain_order=True)
        .with_row_index("row")
    )
    frames = {
        "payments": payments.drop("pay_key"),
        "claims": claims.drop("pay_key", "claim_key"),
        "services": services.drop("service_key", "claim_key"),
        "adjustments": adjustments.drop("claim_key", "service_key"),
        "provider_adjustments": provider_adjustments.drop("pay_key"),
    }
    data = oxedi.write(frames, envelope)
    ```

=== "SQL"

    The examples need `my_*` tables. To try them, build them from the tables of a file, with
    keys of your own in place of the positions. When the extension is not loaded yet:

    <!-- docs-test: skip the runner loads the locally built extension; INSTALL needs the network -->
    ```sql
    INSTALL oxedi FROM community;  -- once
    LOAD oxedi;
    ```

    ```sql
    CREATE TABLE my_payments AS
      SELECT "row" + 1 AS pay_key, * EXCLUDE ("row", segment)
      FROM read_835('remittance.835', table_name = 'payments');
    CREATE TABLE my_claims AS
      SELECT "row" + 1 AS claim_key, payment + 1 AS pay_key, * EXCLUDE ("row", segment, payment)
      FROM read_835('remittance.835', table_name = 'claims');
    CREATE TABLE my_services AS
      SELECT "row" + 1 AS service_key, claim + 1 AS claim_key,
             * EXCLUDE ("row", segment, payment, claim)
      FROM read_835('remittance.835', table_name = 'services');
    CREATE TABLE my_adjustments AS
      SELECT claim + 1 AS claim_key, service + 1 AS service_key,
             * EXCLUDE ("row", segment, payment, claim, service)
      FROM read_835('remittance.835', table_name = 'adjustments');
    CREATE TABLE my_provider_adjustments AS
      SELECT payment + 1 AS pay_key, * EXCLUDE ("row", segment, payment)
      FROM read_835('remittance.835', table_name = 'provider_adjustments');
    ```

    Give each table a `row` with `row_number()`. Put the parents' `row` in its parent columns
    with a join, and drop your helper columns with `EXCLUDE`.

    - Number each child table ordered by the parent's `row`, then by its own key, then by a
      stable tie-breaker: a sequence column, or `rowid` of a table.
    - The adjustments of a claim, which have no `service`, come first (`NULLS FIRST`).
    - Your keys need not sort in parent order across parents.

    ```sql
    COPY (
      WITH payments AS (
             SELECT row_number() OVER (ORDER BY pay_key) - 1 AS "row", * FROM my_payments),
           claims AS (
             SELECT row_number() OVER (ORDER BY p."row", c.claim_key) - 1 AS "row", p."row" AS payment, c.*
             FROM my_claims c JOIN payments p USING (pay_key)),
           services AS (
             SELECT row_number() OVER (ORDER BY c."row", s.service_key) - 1 AS "row",
                    c.payment, c."row" AS claim, s.*
             FROM my_services s JOIN claims c USING (claim_key)),
           adjustments AS (
             SELECT row_number() OVER (ORDER BY c."row", s."row" NULLS FIRST, a.rowid) - 1 AS "row",
                    c.payment, c."row" AS claim, s."row" AS service, a.*
             FROM my_adjustments a
             JOIN claims c USING (claim_key)
             LEFT JOIN services s ON s.service_key = a.service_key),
           provider_adjustments AS (
             SELECT row_number() OVER (ORDER BY p."row", a.rowid) - 1 AS "row", p."row" AS payment, a.*
             FROM my_provider_adjustments a JOIN payments p USING (pay_key))
      SELECT {
        'payments': (SELECT list(t ORDER BY t."row") FROM (SELECT * EXCLUDE (pay_key) FROM payments) t),
        'claims': (SELECT list(t ORDER BY t."row")
                   FROM (SELECT * EXCLUDE (pay_key, claim_key) FROM claims) t),
        'services': (SELECT list(t ORDER BY t."row")
                     FROM (SELECT * EXCLUDE (service_key, claim_key) FROM services) t),
        'adjustments': (SELECT list(t ORDER BY t."row")
                        FROM (SELECT * EXCLUDE (claim_key, service_key) FROM adjustments) t),
        'provider_adjustments': (SELECT list(t ORDER BY t."row")
                                 FROM (SELECT * EXCLUDE (pay_key) FROM provider_adjustments) t)
      }
    ) TO 'out.835' (FORMAT edi835, sender_id 'ACMEPAYER', receiver_id 'SUNRISECLINIC',
                    date DATE '2024-01-10', time TIME '09:00');
    ```
