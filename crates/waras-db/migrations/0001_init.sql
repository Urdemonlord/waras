-- WARAS Fase 1 — 0001_init.sql
-- Semua tabel Fase 1 sekaligus (repo baru). SQLite, kolom snake_case.
-- Tanggal disimpan sebagai TEXT 'YYYY-MM-DD', timestamp 'YYYY-MM-DD HH:MM:SS'.
-- Enum disimpan sebagai TEXT snake_case (lihat waras-db/src/convert.rs).

CREATE TABLE IF NOT EXISTS users (
    id            TEXT PRIMARY KEY,
    nip           TEXT NOT NULL UNIQUE,
    username      TEXT NOT NULL UNIQUE,
    name          TEXT NOT NULL,
    role          TEXT NOT NULL,
    active        INTEGER NOT NULL DEFAULT 1,
    password_hash TEXT NOT NULL,
    created_at    TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS sessions (
    token      TEXT PRIMARY KEY,
    user_id    TEXT NOT NULL REFERENCES users(id),
    created_at TEXT NOT NULL,
    expires_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS patients (
    id           TEXT PRIMARY KEY,
    nik          TEXT NOT NULL,
    name         TEXT NOT NULL,
    birth_date   TEXT NOT NULL,
    gender       TEXT NOT NULL,
    address      TEXT NOT NULL,
    phone        TEXT,
    blood_type   TEXT,
    bpjs_number  TEXT,
    insurance    TEXT NOT NULL,
    created_at   TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS allergies (
    id              TEXT PRIMARY KEY,
    patient_id      TEXT NOT NULL REFERENCES patients(id),
    substance       TEXT NOT NULL,
    substance_class TEXT,
    criticality     TEXT NOT NULL,
    note            TEXT,
    recorded_at     TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS encounters (
    id          TEXT PRIMARY KEY,
    patient_id  TEXT NOT NULL REFERENCES patients(id),
    poli        TEXT NOT NULL,
    doctor_id   TEXT REFERENCES users(id),
    doctor_name TEXT NOT NULL,
    date        TEXT NOT NULL,
    time        TEXT NOT NULL,
    status      TEXT NOT NULL,
    created_at  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS observations (
    id             TEXT PRIMARY KEY,
    encounter_id   TEXT NOT NULL REFERENCES encounters(id),
    patient_id     TEXT NOT NULL REFERENCES patients(id),
    code           TEXT NOT NULL,
    value          TEXT NOT NULL,
    unit           TEXT NOT NULL,
    measured_at    TEXT NOT NULL,
    measured_by    TEXT NOT NULL,
    previous_value TEXT,
    previous_date  TEXT
);

CREATE TABLE IF NOT EXISTS soap_notes (
    id             TEXT PRIMARY KEY,
    encounter_id   TEXT NOT NULL UNIQUE REFERENCES encounters(id),
    subjective     TEXT NOT NULL DEFAULT '',
    objective      TEXT NOT NULL DEFAULT '',
    assessment     TEXT NOT NULL DEFAULT '',
    plan           TEXT NOT NULL DEFAULT '',
    author         TEXT NOT NULL,
    status         TEXT NOT NULL,
    version        INTEGER NOT NULL DEFAULT 1,
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL,
    draft_saved_at TEXT
);

CREATE TABLE IF NOT EXISTS soap_versions (
    id          TEXT PRIMARY KEY,
    soap_id     TEXT NOT NULL REFERENCES soap_notes(id),
    version     INTEGER NOT NULL,
    subjective  TEXT NOT NULL DEFAULT '',
    objective   TEXT NOT NULL DEFAULT '',
    assessment  TEXT NOT NULL DEFAULT '',
    plan        TEXT NOT NULL DEFAULT '',
    author      TEXT NOT NULL,
    saved_at    TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS conditions (
    id              TEXT PRIMARY KEY,
    patient_id      TEXT NOT NULL REFERENCES patients(id),
    encounter_id    TEXT REFERENCES encounters(id),
    condition_group TEXT NOT NULL,
    code            TEXT NOT NULL,
    display         TEXT NOT NULL,
    status          TEXT NOT NULL,
    clinician       TEXT NOT NULL,
    recorded_at     TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS medication_requests (
    id             TEXT PRIMARY KEY,
    encounter_id   TEXT NOT NULL REFERENCES encounters(id),
    patient_id     TEXT NOT NULL REFERENCES patients(id),
    drug_name      TEXT NOT NULL,
    drug_class     TEXT NOT NULL,
    dosage         TEXT NOT NULL,
    duration       TEXT NOT NULL,
    status         TEXT NOT NULL,
    doctor         TEXT NOT NULL,
    blocked_reason TEXT,
    replaced_by    TEXT,
    created_at     TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS drugs (
    kfa_code   TEXT PRIMARY KEY,
    name       TEXT NOT NULL,
    drug_class TEXT NOT NULL,
    form       TEXT NOT NULL,
    stock      INTEGER NOT NULL,
    location   TEXT NOT NULL,
    expiry     TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS queue_entries (
    id         TEXT PRIMARY KEY,
    number     TEXT NOT NULL,
    patient_id TEXT NOT NULL REFERENCES patients(id),
    poli       TEXT NOT NULL,
    doctor     TEXT NOT NULL,
    date       TEXT NOT NULL,
    time       TEXT NOT NULL,
    status     TEXT NOT NULL,
    called_at  TEXT,
    UNIQUE(date, number)
);

CREATE TABLE IF NOT EXISTS appointments (
    id                TEXT PRIMARY KEY,
    patient_id        TEXT NOT NULL REFERENCES patients(id),
    poli              TEXT NOT NULL,
    date              TEXT NOT NULL,
    appointment_time  TEXT NOT NULL,
    consultation_time TEXT NOT NULL,
    status            TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS invoices (
    id           TEXT PRIMARY KEY,
    patient_id   TEXT NOT NULL REFERENCES patients(id),
    encounter_id TEXT REFERENCES encounters(id),
    payer        TEXT NOT NULL,
    service      TEXT NOT NULL,
    amount       INTEGER NOT NULL,
    status       TEXT NOT NULL,
    ina_cbg_code TEXT,
    created_at   TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS payments (
    id         TEXT PRIMARY KEY,
    invoice_id TEXT NOT NULL REFERENCES invoices(id),
    method     TEXT NOT NULL,
    amount     INTEGER NOT NULL,
    cashier    TEXT NOT NULL,
    paid_at    TEXT NOT NULL
);

-- Append-only: tidak ada UPDATE/DELETE di kode aplikasi.
CREATE TABLE IF NOT EXISTS audit_log (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp     TEXT NOT NULL,
    actor         TEXT NOT NULL,
    action        TEXT NOT NULL,
    resource_type TEXT NOT NULL,
    resource_id   TEXT NOT NULL,
    details       TEXT NOT NULL DEFAULT '{}',
    prev_hash     TEXT NOT NULL,
    entry_hash    TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS daily_stats (
    date           TEXT PRIMARY KEY,
    registrations  INTEGER NOT NULL,
    visits         INTEGER NOT NULL,
    revenue_idr    INTEGER NOT NULL,
    transactions   INTEGER NOT NULL,
    lab_pending    INTEGER NOT NULL,
    lab_critical   INTEGER NOT NULL,
    emergency_red  INTEGER NOT NULL,
    beds_occupied  INTEGER NOT NULL,
    beds_total     INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS notifications (
    id      TEXT PRIMARY KEY,
    kind    TEXT NOT NULL,
    message TEXT NOT NULL,
    time    TEXT NOT NULL,
    date    TEXT NOT NULL,
    read    INTEGER NOT NULL DEFAULT 0
);
