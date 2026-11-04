//! Unit tests of the spec module, one file per topic.

mod balancing;
mod builtin;
mod codes;
mod columns;
mod controls;
mod display;
mod loading;
mod occurrences;
mod patch;
mod phi;
mod segments;
mod shape;
mod summaries;
mod tables;
mod triggers;
mod version;

use crate::segment::Segment;
use crate::spec::{Spec, SpecError};
use crate::{Delimiters, Tokenizer};

pub(super) fn segs(input: &[u8]) -> Vec<Segment<'_>> {
    Tokenizer::with_delimiters(input, Delimiters::new(b'*', b':', b'~')).collect()
}

pub(super) const CLP_ONLY: &str = r#"{"name":"t",
        "loops":{"2100":{"trigger":{"segment":"CLP"},"occurrences":{"clp":{"segment":"CLP","pos":0},"zz1":{"segment":"ZZ1","pos":1}}}},
        "segments":{"CLP":{"elements":{
            "1":{"name":"claim_submitter_id","type":"AN","required":true,"min":1,"max":38},
            "3":{"name":"total_claim_charge_amount","type":"R","required":true},
            "12":{"name":"drg_weight","type":"R","scale":4}
        }},
        "SVC":{"elements":{
            "1":{"name":"procedure","type":"AN","required":true,"composite":{
                "1":{"name":"qualifier","type":"ID","required":true,"min":2,"max":2},
                "2":{"name":"code","type":"AN","required":true}
            }},
            "5":{"name":"units","type":"N0"}
        }}}
    }"#;

pub(super) fn element_error(elements: &str) -> SpecError {
    let json = format!(
        r#"{{"name":"t","loops":{{"a":{{"trigger":{{"segment":"AA"}}}}}},"segments":{{"AA":{{"elements":{elements}}}}}}}"#
    );
    Spec::from_json(&json).unwrap_err()
}

pub(super) const TABLED: &str = r#"{"name":"t",
        "loops":{
            "A":{"trigger":{"segment":"AA"},"occurrences":{"aa":{"segment":"AA","pos":0},"a1":{"segment":"A1","pos":1}},"end":"AE"},
            "B":{"parent":"A","trigger":{"segment":"BB"},"occurrences":{"bb":{"segment":"BB","pos":0},"b1":{"segment":"B1","pos":1},"aj":{"segment":"AJ","pos":2}}},
            "C":{"parent":"B","trigger":{"segment":"CC"},"occurrences":{"cc":{"segment":"CC","pos":0},"c1":{"segment":"C1","pos":1},"aj":{"segment":"AJ","pos":2}}},
            "D":{"parent":"A","trigger":{"segment":"DD"}}
        },
        "segments":{
            "BB":{"elements":{"1":{"name":"id","type":"AN"},"2":{"name":"amount","type":"R"}}},
            "CC":{"elements":{"1":{"name":"code","type":"AN","composite":{
                "1":{"name":"qualifier","type":"ID"},"2":{"name":"value","type":"AN"}}}}}
        },
        "tables":{
            "heads":{"loops":["A"],"ref":"head","columns":{
                "code":{"segment":"AA","element":1},
                "note":{"loop":"D","segment":"DD","where":{"1":"N"},"element":2}
            }},
            "bodies":{"loops":["B"],"ref":"body","columns":{
                "id":{"segment":"BB","element":1},
                "line_at":{"loop":"C","segment":"C1","segment_index":true}
            }},
            "lines":{"loops":["C"],"ref":"line","columns":{
                "code":{"segment":"CC","element":1,"component":2}
            }},
            "adjustments":{"loops":["B","C"],"segment":"AJ","repeat":{"from":2,"step":2},"columns":{
                "kind":{"element":1},
                "reason":{"group_element":0},
                "amount":{"group_element":1}
            }}
        }
    }"#;

pub(super) fn table_error(tables: &str) -> SpecError {
    let json = format!(
        r#"{{"name":"t","loops":{{
                "A":{{"trigger":{{"segment":"AA"}}}},
                "B":{{"parent":"A","trigger":{{"segment":"BB"}}}},
                "C":{{"parent":"B","trigger":{{"segment":"CC"}},"occurrences":{{"cc":{{"segment":"CC","pos":0}},"xx":{{"segment":"XX","pos":1}}}}}},
                "D":{{"parent":"A","trigger":{{"segment":"DD"}},"occurrences":{{"dd":{{"segment":"DD","pos":0}},"xx":{{"segment":"XX","pos":1}}}}}}
            }},"tables":{tables}}}"#
    );
    Spec::from_json(&json).unwrap_err()
}
