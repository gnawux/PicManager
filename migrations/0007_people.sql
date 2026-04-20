CREATE TABLE IF NOT EXISTS people (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    name          TEXT,                             -- 用户自定义名称，NULL = 未命名
    parent_id     INTEGER REFERENCES people(id),   -- NULL = 顶级人物；非 NULL = 子节点
    cover_face_id INTEGER REFERENCES faces(id),    -- 代表性人脸；NULL 时前端取第一张
    created_at    TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS person_faces (
    person_id INTEGER NOT NULL REFERENCES people(id) ON DELETE CASCADE,
    face_id   INTEGER NOT NULL REFERENCES faces(id)  ON DELETE CASCADE,
    PRIMARY KEY (person_id, face_id)
);
