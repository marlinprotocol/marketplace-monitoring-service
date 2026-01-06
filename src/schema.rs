diesel::table! {
    bsc_reachability_errors (id) {
        id -> Int4,
        job -> Varchar,
        operator -> Varchar,
        ip -> Varchar,
        error -> Varchar,
        timestamp -> Int8,
    }
}

diesel::table! {
    bsc_operator_errors (id) {
        id -> Int4,
        job -> Varchar,
        operator -> Varchar,
        ip -> Varchar,
        error -> Varchar,
        timestamp -> Int8,
    }
}
