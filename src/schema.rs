diesel::table! {
    reachability_errors (id) {
        id -> Int4,
        job -> Varchar,
        operator -> Varchar,
        ip -> Varchar,
        error -> Varchar,
        timestamp -> Int8,
    }
}

diesel::table! {
    operator_endpoint_errors (id) {
        id -> Int4,
        job -> Varchar,
        operator -> Varchar,
        ip -> Varchar,
        error -> Varchar,
        timestamp -> Int8,
    }
}
