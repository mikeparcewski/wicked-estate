-- codegraph 1.6.4 (darwin-arm64 release bundle, pinned) output for tests/fixtures/codegraph/repo,
-- written by 'codegraph init -y <copy of repo>' and dumped with the sqlite3 shell: the four tables
-- estate reads (files, nodes, edges, unresolved_refs), their exact schema, every row. Regenerate
-- with tests/fixtures/codegraph/regen.sh after any fixture-source change.
CREATE TABLE files (
    path TEXT PRIMARY KEY,
    content_hash TEXT NOT NULL,
    language TEXT NOT NULL,
    size INTEGER NOT NULL,
    modified_at INTEGER NOT NULL,
    indexed_at INTEGER NOT NULL,
    node_count INTEGER DEFAULT 0,
    errors TEXT, -- JSON array
    generated INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE nodes (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    name TEXT NOT NULL,
    qualified_name TEXT NOT NULL,
    file_path TEXT NOT NULL,
    language TEXT NOT NULL,
    start_line INTEGER NOT NULL,
    end_line INTEGER NOT NULL,
    start_column INTEGER NOT NULL,
    end_column INTEGER NOT NULL,
    docstring TEXT,
    signature TEXT,
    visibility TEXT,
    is_exported INTEGER DEFAULT 0,
    is_async INTEGER DEFAULT 0,
    is_static INTEGER DEFAULT 0,
    is_abstract INTEGER DEFAULT 0,
    decorators TEXT, -- JSON array
    type_parameters TEXT, -- JSON array
    return_type TEXT, -- normalized return/result type name (e.g. C++ method return, for receiver-type inference)
    updated_at INTEGER NOT NULL
);
CREATE TABLE edges (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source TEXT NOT NULL,
    target TEXT NOT NULL,
    kind TEXT NOT NULL,
    metadata TEXT, -- JSON object
    line INTEGER,
    col INTEGER,
    provenance TEXT DEFAULT NULL,
    FOREIGN KEY (source) REFERENCES nodes(id) ON DELETE CASCADE,
    FOREIGN KEY (target) REFERENCES nodes(id) ON DELETE CASCADE
);
CREATE TABLE unresolved_refs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    from_node_id TEXT NOT NULL,
    reference_name TEXT NOT NULL,
    reference_kind TEXT NOT NULL,
    line INTEGER NOT NULL,
    col INTEGER NOT NULL,
    candidates TEXT, -- JSON array
    file_path TEXT NOT NULL DEFAULT '',
    language TEXT NOT NULL DEFAULT 'unknown',
    status TEXT NOT NULL DEFAULT 'pending',
    name_tail TEXT NOT NULL DEFAULT '',
    FOREIGN KEY (from_node_id) REFERENCES nodes(id) ON DELETE CASCADE
);
INSERT INTO files VALUES('src/lib/util.ts','8d398181201144d57fe4b4c5fcf898bb88d0eabad947de6b0335290e391ce304','typescript',134,1795492940070.606933,1795492949109,3,NULL,0);
INSERT INTO files VALUES('src/p/Shapes.java','99f11d73467974ace908a3fea6f62ba40ab2e57f6898ee8f864546081d112cfd','java',197,1795492940079.205811,1795492949110,10,NULL,0);
INSERT INTO files VALUES('src/same.ts','7ee5ce65dcb074b77590aca3ba2d0a29dd5c55de2102941967cd4ef3ba9684b6','typescript',167,1795492940068.841552,1795492949111,6,NULL,0);
INSERT INTO files VALUES('src/svc.ts','f9f5414897276caf12cbb368731cd2489ff0c2b9890bc5b3f70ed43fe3dbd744','typescript',262,1795492940069.401123,1795492949114,6,NULL,0);
INSERT INTO files VALUES('src/util.ts','4b73a45a7f18ad33dfcf41110ec47258df443b62ecf1b6abf49cc7c8a7c82dee','typescript',153,1795492940071.369874,1795492949114,2,NULL,0);
INSERT INTO nodes VALUES('file:src/lib/util.ts','file','util.ts','src/lib/util.ts','src/lib/util.ts','typescript',1,3,0,0,NULL,NULL,NULL,0,0,0,0,NULL,NULL,NULL,1795492949109);
INSERT INTO nodes VALUES('function:a1f6ebd3c9bfbcb942ebc1692bc3d480','function','helper','helper','src/lib/util.ts','typescript',1,1,7,69,NULL,'(s: string): string',NULL,1,0,0,0,NULL,NULL,NULL,1795492949109);
INSERT INTO nodes VALUES('function:8aae4a3959a7696dfc201b77a7cfd024','function','format','format','src/lib/util.ts','typescript',2,2,7,63,NULL,'(n: number): string',NULL,1,0,0,0,NULL,NULL,NULL,1795492949109);
INSERT INTO nodes VALUES('file:src/p/Shapes.java','file','Shapes.java','src/p/Shapes.java','src/p/Shapes.java','java',1,8,0,0,NULL,NULL,NULL,0,0,0,0,NULL,NULL,NULL,1795492949113);
INSERT INTO nodes VALUES('namespace:5ffd9439c6c4e7558cf5460b164f1f05','namespace','p','p','src/p/Shapes.java','java',1,1,0,10,NULL,NULL,NULL,0,0,0,0,NULL,NULL,NULL,1795492949113);
INSERT INTO nodes VALUES('class:24c5e42ecf936a2beb512baf5d879067','class','Shapes','p::Shapes','src/p/Shapes.java','java',2,7,0,1,NULL,NULL,'public',0,0,0,0,NULL,NULL,NULL,1795492949113);
INSERT INTO nodes VALUES('method:a272bd91777c853fafe660afa6501ff6','method','f','p::Shapes::f','src/p/Shapes.java','java',3,3,2,28,NULL,'int (int x)',NULL,0,0,0,0,NULL,NULL,NULL,1795492949113);
INSERT INTO nodes VALUES('method:e9a6208e271a7b5a5649e0d8f256ede7','method','f','p::Shapes::f','src/p/Shapes.java','java',4,4,2,43,NULL,'int (String s)',NULL,0,0,0,0,NULL,NULL,NULL,1795492949113);
INSERT INTO nodes VALUES('class:39d4cae6d10a7215f9b17606b4c1cd8f','class','Point','p::Shapes::Point','src/p/Shapes.java','java',5,5,2,47,NULL,NULL,NULL,0,0,0,0,NULL,NULL,NULL,1795492949113);
INSERT INTO nodes VALUES('method:4f049573e60a8dde15a99d8b7fafb5f3','method','g','p::Shapes::Point::g','src/p/Shapes.java','java',5,5,24,45,NULL,'int ()',NULL,0,0,0,0,NULL,NULL,NULL,1795492949113);
INSERT INTO nodes VALUES('field:ee5544be799c3d3897d9a24ca0b0b04f','field','a','p::Shapes::Point::a','src/p/Shapes.java','java',5,5,19,20,NULL,'int a','private',0,0,0,0,NULL,NULL,NULL,1795492949113);
INSERT INTO nodes VALUES('method:02d470d51029f9054e07bf4324d3d90a','method','a','p::Shapes::Point::a','src/p/Shapes.java','java',5,5,19,20,'Implicit record component accessor','int a()','public',0,0,0,0,NULL,NULL,NULL,1795492949113);
INSERT INTO nodes VALUES('method:6fd9c5d6fb6b1115201d9033fc7f57a2','method','use','p::Shapes::use','src/p/Shapes.java','java',6,6,2,40,NULL,'int ()',NULL,0,0,0,0,NULL,NULL,NULL,1795492949113);
INSERT INTO nodes VALUES('file:src/same.ts','file','same.ts','src/same.ts','src/same.ts','typescript',1,3,0,0,NULL,NULL,NULL,0,0,0,0,NULL,NULL,NULL,1795492949114);
INSERT INTO nodes VALUES('class:693938a95fba5282ba7db4c50f728502','class','A','A','src/same.ts','typescript',1,1,7,46,NULL,NULL,NULL,1,0,0,0,NULL,NULL,NULL,1795492949114);
INSERT INTO nodes VALUES('method:b57287721800de69d834730d94a6e02f','method','run','A::run','src/same.ts','typescript',1,1,17,44,NULL,'(): number',NULL,0,0,0,0,NULL,NULL,NULL,1795492949114);
INSERT INTO nodes VALUES('class:585f099136afc6dd1dfae4cc34393a08','class','B','B','src/same.ts','typescript',1,1,54,93,NULL,NULL,NULL,1,0,0,0,NULL,NULL,NULL,1795492949114);
INSERT INTO nodes VALUES('method:b57287721800de69d834730d94a6e02f:64','method','run','B::run','src/same.ts','typescript',1,1,64,91,NULL,'(): number',NULL,0,0,0,0,NULL,NULL,NULL,1795492949114);
INSERT INTO nodes VALUES('function:f1b34979e8483a414e6e2aa7fc4161a6','function','both','both','src/same.ts','typescript',2,2,7,72,NULL,'(): number',NULL,1,0,0,0,NULL,NULL,NULL,1795492949114);
INSERT INTO nodes VALUES('file:src/svc.ts','file','svc.ts','src/svc.ts','src/svc.ts','typescript',1,7,0,0,NULL,NULL,NULL,0,0,0,0,NULL,NULL,NULL,1795492949114);
INSERT INTO nodes VALUES('import:8763fb8912d02f5f8dc3ef49c8f891da','import','@lib/util','@lib/util','src/svc.ts','typescript',1,1,0,35,NULL,'import { helper } from ''@lib/util'';',NULL,0,0,0,0,NULL,NULL,NULL,1795492949114);
INSERT INTO nodes VALUES('class:31b8bf0e699b66ea01bd731cc7a262bc','class','UserService','UserService','src/svc.ts','typescript',2,5,7,1,NULL,NULL,NULL,1,0,0,0,NULL,NULL,NULL,1795492949114);
INSERT INTO nodes VALUES('method:e932f64ca317362f4f37a065ac052f2a','method','get','UserService::get','src/svc.ts','typescript',3,3,2,55,NULL,'(id: string): string',NULL,0,0,0,0,NULL,NULL,NULL,1795492949114);
INSERT INTO nodes VALUES('method:ebe50503bdd6103a8e43398cc377ab6c','method','get2','UserService::get2','src/svc.ts','typescript',4,4,2,59,NULL,'(id: number): string',NULL,0,0,0,0,NULL,NULL,NULL,1795492949114);
INSERT INTO nodes VALUES('function:57f503dd9606a92169ff7e2af2c870d3','function','top','top','src/svc.ts','typescript',6,6,7,80,NULL,'(x: string)',NULL,1,0,0,0,NULL,NULL,NULL,1795492949114);
INSERT INTO nodes VALUES('file:src/util.ts','file','util.ts','src/util.ts','src/util.ts','typescript',1,3,0,0,NULL,NULL,NULL,0,0,0,0,NULL,NULL,NULL,1795492949114);
INSERT INTO nodes VALUES('function:49c99c46fb86edd4958a019e33d9a2bb','function','helper','helper','src/util.ts','typescript',2,2,7,55,'A same-named decoy: estate''s name resolver binds `helper` here or to src/lib/util.ts by name.','(s: string): string',NULL,1,0,0,0,NULL,NULL,NULL,1795492949114);
INSERT INTO edges VALUES(1,'file:src/lib/util.ts','function:a1f6ebd3c9bfbcb942ebc1692bc3d480','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(2,'file:src/lib/util.ts','function:8aae4a3959a7696dfc201b77a7cfd024','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(3,'file:src/p/Shapes.java','namespace:5ffd9439c6c4e7558cf5460b164f1f05','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(4,'namespace:5ffd9439c6c4e7558cf5460b164f1f05','class:24c5e42ecf936a2beb512baf5d879067','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(5,'class:24c5e42ecf936a2beb512baf5d879067','method:a272bd91777c853fafe660afa6501ff6','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(6,'class:24c5e42ecf936a2beb512baf5d879067','method:e9a6208e271a7b5a5649e0d8f256ede7','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(7,'class:24c5e42ecf936a2beb512baf5d879067','class:39d4cae6d10a7215f9b17606b4c1cd8f','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(8,'class:39d4cae6d10a7215f9b17606b4c1cd8f','method:4f049573e60a8dde15a99d8b7fafb5f3','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(9,'class:39d4cae6d10a7215f9b17606b4c1cd8f','field:ee5544be799c3d3897d9a24ca0b0b04f','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(10,'class:39d4cae6d10a7215f9b17606b4c1cd8f','method:02d470d51029f9054e07bf4324d3d90a','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(11,'class:24c5e42ecf936a2beb512baf5d879067','method:6fd9c5d6fb6b1115201d9033fc7f57a2','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(12,'file:src/same.ts','class:693938a95fba5282ba7db4c50f728502','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(13,'class:693938a95fba5282ba7db4c50f728502','method:b57287721800de69d834730d94a6e02f','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(14,'file:src/same.ts','class:585f099136afc6dd1dfae4cc34393a08','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(15,'class:585f099136afc6dd1dfae4cc34393a08','method:b57287721800de69d834730d94a6e02f:64','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(16,'file:src/same.ts','function:f1b34979e8483a414e6e2aa7fc4161a6','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(17,'file:src/svc.ts','import:8763fb8912d02f5f8dc3ef49c8f891da','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(18,'file:src/svc.ts','class:31b8bf0e699b66ea01bd731cc7a262bc','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(19,'class:31b8bf0e699b66ea01bd731cc7a262bc','method:e932f64ca317362f4f37a065ac052f2a','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(20,'class:31b8bf0e699b66ea01bd731cc7a262bc','method:ebe50503bdd6103a8e43398cc377ab6c','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(21,'file:src/svc.ts','function:57f503dd9606a92169ff7e2af2c870d3','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(22,'file:src/util.ts','function:49c99c46fb86edd4958a019e33d9a2bb','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(23,'file:src/svc.ts','file:src/lib/util.ts','imports','{"confidence":0.9,"resolvedBy":"import","refName":"@lib/util"}',1,0,NULL);
INSERT INTO edges VALUES(24,'file:src/svc.ts','function:a1f6ebd3c9bfbcb942ebc1692bc3d480','imports','{"confidence":0.9,"resolvedBy":"import","refName":"helper"}',1,9,NULL);
INSERT INTO edges VALUES(25,'method:e9a6208e271a7b5a5649e0d8f256ede7','method:e9a6208e271a7b5a5649e0d8f256ede7','calls','{"confidence":0.7,"resolvedBy":"exact-match","refName":"f"}',4,27,NULL);
INSERT INTO edges VALUES(26,'method:6fd9c5d6fb6b1115201d9033fc7f57a2','class:39d4cae6d10a7215f9b17606b4c1cd8f','instantiates','{"confidence":0.9,"resolvedBy":"exact-match","refName":"Point"}',6,21,NULL);
INSERT INTO edges VALUES(27,'function:f1b34979e8483a414e6e2aa7fc4161a6','method:b57287721800de69d834730d94a6e02f','calls','{"confidence":0.9,"resolvedBy":"instance-method","refName":"new A().run"}',2,40,NULL);
INSERT INTO edges VALUES(28,'function:f1b34979e8483a414e6e2aa7fc4161a6','class:693938a95fba5282ba7db4c50f728502','instantiates','{"confidence":0.9,"resolvedBy":"exact-match","refName":"A"}',2,40,NULL);
INSERT INTO edges VALUES(29,'function:f1b34979e8483a414e6e2aa7fc4161a6','method:b57287721800de69d834730d94a6e02f:64','calls','{"confidence":0.9,"resolvedBy":"instance-method","refName":"new B().run"}',2,56,NULL);
INSERT INTO edges VALUES(30,'function:f1b34979e8483a414e6e2aa7fc4161a6','class:585f099136afc6dd1dfae4cc34393a08','instantiates','{"confidence":0.9,"resolvedBy":"exact-match","refName":"B"}',2,56,NULL);
INSERT INTO edges VALUES(31,'method:e932f64ca317362f4f37a065ac052f2a','function:a1f6ebd3c9bfbcb942ebc1692bc3d480','calls','{"confidence":0.9,"resolvedBy":"import","refName":"helper"}',3,35,NULL);
INSERT INTO edges VALUES(32,'method:ebe50503bdd6103a8e43398cc377ab6c','method:e932f64ca317362f4f37a065ac052f2a','calls','{"confidence":0.9,"resolvedBy":"exact-match","refName":"get"}',4,36,NULL);
INSERT INTO edges VALUES(33,'function:57f503dd9606a92169ff7e2af2c870d3','class:31b8bf0e699b66ea01bd731cc7a262bc','instantiates','{"confidence":0.9,"resolvedBy":"exact-match","refName":"UserService"}',6,43,NULL);
INSERT INTO edges VALUES(34,'function:57f503dd9606a92169ff7e2af2c870d3','method:e932f64ca317362f4f37a065ac052f2a','calls','{"confidence":0.9,"resolvedBy":"instance-method","refName":"s.get"}',6,69,NULL);
INSERT INTO unresolved_refs VALUES(1,'function:a1f6ebd3c9bfbcb942ebc1692bc3d480','s.toUpperCase','calls',1,51,NULL,'src/lib/util.ts','typescript','failed','toUpperCase');
INSERT INTO unresolved_refs VALUES(2,'function:8aae4a3959a7696dfc201b77a7cfd024','String','calls',2,51,NULL,'src/lib/util.ts','typescript','failed','String');
INSERT INTO unresolved_refs VALUES(4,'method:e9a6208e271a7b5a5649e0d8f256ede7','s.length','calls',4,29,NULL,'src/p/Shapes.java','java','failed','length');
INSERT INTO unresolved_refs VALUES(5,'method:6fd9c5d6fb6b1115201d9033fc7f57a2','new Point(1).g','calls',6,21,NULL,'src/p/Shapes.java','java','failed','g');
INSERT INTO unresolved_refs VALUES(13,'method:e932f64ca317362f4f37a065ac052f2a','helper().trim','calls',3,35,NULL,'src/svc.ts','typescript','failed','trim');
INSERT INTO unresolved_refs VALUES(16,'method:ebe50503bdd6103a8e43398cc377ab6c','String','calls',4,45,NULL,'src/svc.ts','typescript','failed','String');
