//! A small spec with four tables, shared by the unit tests of the spec and the
//! projector.

use super::Spec;

pub(crate) const SPEC: &str = r#"{"name":"t",
    "loops":{
        "head":{"trigger":{"segment":"HD"},"occurrences":{"hd":{"segment":"HD","pos":0},"zz":{"segment":"ZZ","pos":1}},"end":"TR"},
        "note":{"parent":"head","trigger":{"segment":"NM","where":{"1":"P"}}},
        "claim":{"parent":"head","trigger":{"segment":"CL"},"occurrences":{"cl":{"segment":"CL","pos":0},"dt":{"segment":"DT","pos":1},"aj":{"segment":"AJ","pos":2},"rf":{"segment":"RF","pos":3}}},
        "line":{"parent":"claim","trigger":{"segment":"LN"},"occurrences":{"ln":{"segment":"LN","pos":0},"dt":{"segment":"DT","pos":1},"aj":{"segment":"AJ","pos":2}}}
    },
    "segments":{
        "HD":{"elements":{"1":{"name":"batch","type":"AN","required":true,"min":1,"max":5}}},
        "CL":{"elements":{
            "1":{"name":"claim_id","type":"AN","required":true,"min":1,"max":10},
            "2":{"name":"charge","type":"R","required":true,"max":10},
            "3":{"name":"units","type":"N0","max":2},
            "4":{"name":"procedure","type":"AN","composite":{
                "1":{"name":"qualifier","type":"ID","required":true,"min":2,"max":2},
                "2":{"name":"code","type":"AN","required":true,"max":5}
            }}
        }},
        "DT":{"elements":{
            "1":{"name":"qualifier","type":"ID","required":true},
            "2":{"name":"date","type":"DT"},
            "3":{"name":"time","type":"TM"}
        }},
        "AJ":{"elements":{
            "1":{"name":"group","type":"ID","required":true},
            "2":{"name":"reason","type":"ID"},
            "3":{"name":"amount","type":"R"},
            "4":{"name":"reason_2","type":"ID"},
            "5":{"name":"amount_2","type":"R"}
        }}
    },
    "tables":{
        "heads":{"loops":["head"],"ref":"head","columns":{
            "batch":{"segment":"HD","element":1},
            "payer":{"loop":"note","segment":"NM","element":2}
        }},
        "claims":{"loops":["claim"],"ref":"claim","columns":{
            "claim_id":{"segment":"CL","element":1},
            "charge":{"segment":"CL","element":2},
            "units":{"segment":"CL","element":3},
            "procedure":{"segment":"CL","element":4},
            "code":{"segment":"CL","element":4,"component":2},
            "from":{"segment":"DT","where":{"1":"150"},"element":2},
            "reference":{"segment":"RF","element":2},
            "first_line_at":{"loop":"line","segment":"LN","segment_index":true}
        }},
        "lines":{"loops":["line"],"ref":"line","columns":{
            "code":{"segment":"LN","element":1},
            "date":{"segment":"DT","element":2},
            "time":{"segment":"DT","element":3}
        }},
        "adjustments":{"loops":["claim","line"],"segment":"AJ","repeat":{"from":2,"step":2},"columns":{
            "group":{"element":1},
            "reason":{"group_element":0},
            "amount":{"group_element":1}
        }}
    }
}"#;

pub(crate) fn spec() -> Spec {
    Spec::from_json(SPEC).unwrap()
}
