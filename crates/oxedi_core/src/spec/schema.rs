//! The JSON schema of a spec as data: one [`ObjectSchema`] per object the
//! spec holds, each listing its keys in the order their values are checked.
//! [`ROOT`] lists the sections of a spec; `shape` walks a value against it.

use super::shape::{Field, Leaf, ObjectSchema};

const fn leaf(kind: Leaf) -> Field {
    Field::Leaf {
        kind,
        nullable: false,
    }
}

const fn nullable(kind: Leaf) -> Field {
    Field::Leaf {
        kind,
        nullable: true,
    }
}

const TEXT: Field = leaf(Leaf::Text);
const COUNT: Field = leaf(Leaf::Count);

const TEXTS: Field = Field::List {
    item: &TEXT,
    expected: "an array of strings",
};

const COUNTS: Field = Field::List {
    item: &COUNT,
    expected: "an array of non-negative integers",
};

/// An array of objects of one schema.
macro_rules! objects {
    ($schema:expr) => {
        Field::List {
            item: &Field::Object($schema),
            expected: "an array of objects",
        }
    };
}

/// The sections of a spec.
pub(super) static ROOT: ObjectSchema = ObjectSchema {
    keys: &[
        ("name", TEXT),
        ("version", Field::Object(&VERSION)),
        ("loops", Field::Map(&Field::Object(&LOOP))),
        ("tables", Field::Map(&Field::Object(&TABLE))),
        ("balancing", Field::Map(&Field::Object(&BALANCE_RULE))),
        ("segments", Field::Map(&Field::Object(&SEGMENT))),
        ("summaries", Field::Map(&Field::Object(&SUMMARY))),
        ("default_table", TEXT),
    ],
    required: &["name", "loops"],
};

static VERSION: ObjectSchema = ObjectSchema {
    keys: &[("segment", TEXT), ("element", COUNT), ("values", TEXTS)],
    required: &["segment", "element", "values"],
};

static LOOP: ObjectSchema = ObjectSchema {
    keys: &[
        ("parent", nullable(Leaf::Text)),
        ("end", nullable(Leaf::Text)),
        ("max", nullable(Leaf::Count)),
        ("usage", nullable(Leaf::Text)),
        ("occurrences", Field::Map(&Field::Object(&OCCURRENCE))),
        ("trigger", Field::Object(&TRIGGER)),
        ("control", Field::Object(&CONTROL)),
    ],
    required: &["trigger"],
};

static TRIGGER: ObjectSchema = ObjectSchema {
    keys: &[("segment", TEXT), ("where", Field::Map(&TEXT))],
    required: &["segment"],
};

static CONTROL: ObjectSchema = ObjectSchema {
    keys: &[
        ("opener_element", COUNT),
        ("closer_element", COUNT),
        ("count_element", COUNT),
        ("count", TEXT),
    ],
    required: &["opener_element", "closer_element", "count_element", "count"],
};

static OCCURRENCE: ObjectSchema = ObjectSchema {
    keys: &[
        ("segment", TEXT),
        ("pos", COUNT),
        ("usage", nullable(Leaf::Text)),
        ("max", nullable(Leaf::Count)),
        ("qualifier", Field::Object(&QUALIFIER)),
        ("codes", Field::Map(&TEXTS)),
    ],
    required: &["segment", "pos"],
};

static QUALIFIER: ObjectSchema = ObjectSchema {
    keys: &[
        ("element", COUNT),
        ("component", nullable(Leaf::Count)),
        ("codes", TEXTS),
    ],
    required: &["element", "codes"],
};

static TABLE: ObjectSchema = ObjectSchema {
    keys: &[
        ("loops", TEXTS),
        ("ref", nullable(Leaf::Text)),
        ("segment", nullable(Leaf::Text)),
        ("phi_seed", nullable(Leaf::Text)),
        ("repeat", Field::Object(&REPEAT)),
        ("columns", Field::Map(&Field::Object(&COLUMN))),
    ],
    required: &["loops"],
};

static REPEAT: ObjectSchema = ObjectSchema {
    keys: &[("from", COUNT), ("step", COUNT)],
    required: &["from", "step"],
};

static COLUMN: ObjectSchema = ObjectSchema {
    keys: &[
        ("loop", nullable(Leaf::Text)),
        ("segment", nullable(Leaf::Text)),
        ("where", Field::Map(&TEXT)),
        ("occurrence", nullable(Leaf::Text)),
        ("pick", nullable(Leaf::Pick)),
        ("element", nullable(Leaf::Count)),
        ("component", nullable(Leaf::Count)),
        ("group_element", nullable(Leaf::Count)),
        ("segment_index", leaf(Leaf::Flag)),
        ("phi", nullable(Leaf::Text)),
    ],
    required: &[],
};

static BALANCE_RULE: ObjectSchema = ObjectSchema {
    keys: &[
        ("per", TEXT),
        ("target", objects!(&BALANCE_TERM)),
        ("sum", objects!(&BALANCE_TERM)),
    ],
    required: &["per", "target", "sum"],
};

static BALANCE_TERM: ObjectSchema = ObjectSchema {
    keys: &[
        ("loop", nullable(Leaf::Text)),
        ("occurrence", TEXT),
        ("component", nullable(Leaf::Count)),
        ("sign", nullable(Leaf::Text)),
        ("elements", COUNTS),
    ],
    required: &["occurrence", "elements"],
};

static SEGMENT: ObjectSchema = ObjectSchema {
    keys: &[("elements", Field::Map(&Field::Object(&ELEMENT)))],
    required: &[],
};

static ELEMENT: ObjectSchema = ObjectSchema {
    keys: &[
        ("codes", TEXTS),
        ("name", TEXT),
        ("type", TEXT),
        ("required", leaf(Leaf::Flag)),
        ("min", nullable(Leaf::Count)),
        ("max", nullable(Leaf::Count)),
        ("scale", nullable(Leaf::Byte)),
        ("composite", Field::Map(&Field::Object(&ELEMENT))),
    ],
    required: &["name", "type"],
};

static SUMMARY: ObjectSchema = ObjectSchema {
    keys: &[
        ("count", Field::Object(&SUMMARY_TABLE)),
        ("distinct", Field::Object(&SUMMARY_COLUMN)),
        ("sum", Field::Object(&SUMMARY_COLUMN)),
        ("row", Field::Object(&SUMMARY_ROW)),
    ],
    required: &[],
};

static SUMMARY_TABLE: ObjectSchema = ObjectSchema {
    keys: &[("table", TEXT)],
    required: &["table"],
};

static SUMMARY_COLUMN: ObjectSchema = ObjectSchema {
    keys: &[("table", TEXT), ("column", TEXT)],
    required: &["table", "column"],
};

static SUMMARY_ROW: ObjectSchema = ObjectSchema {
    keys: &[("table", TEXT), ("fields", objects!(&SUMMARY_FIELD))],
    required: &["table", "fields"],
};

static SUMMARY_FIELD: ObjectSchema = ObjectSchema {
    keys: &[("name", TEXT), ("column", TEXT)],
    required: &["name", "column"],
};
