-- codegraph 1.6.4 (darwin-arm64 release bundle, pinned) output for tests/fixtures/codegraph/coverage/repo,
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
INSERT INTO files VALUES('src/ts/big.ts','954f30a9abc05a613733d6fb0af09764733970d090789152d9ff513e76a991be','typescript',1048628,1795619163185.614014,1795619179900,0,'[{"message":"File exceeds max size (1048628 > 1048576)","filePath":"src/ts/big.ts","severity":"warning","code":"size_exceeded"}]',0);
INSERT INTO files VALUES('src/c/util.c','a572261537bc61d5630763ffb6618d58194f0481de32e88e6d1e575d5c40bd8a','c',143,1795619163038.82373,1795619179938,4,NULL,0);
INSERT INTO files VALUES('src/c/util.h','da3c52ad5f3595eeb79c08e8b2c3aa94c03d395dc379076088ad1c1c678b3c3b','c',119,1795619163069.237304,1795619179939,2,NULL,0);
INSERT INTO files VALUES('src/cpp/shape.cpp','4c67a8900c8e7e864c39d2ca3ce9835a07e85ae1004c7646b3ceeb8a4426042a','cpp',301,1795619162837.858154,1795619179948,7,NULL,0);
INSERT INTO files VALUES('src/csharp/Ledger.cs','d14ed24994eebc84fbe2ec8c91dfdc4e786757cbe59b6176b7fbeaca0e608af1','csharp',553,1795619162928.152099,1795619179953,13,NULL,0);
INSERT INTO files VALUES('src/java/cov/Animal.java','8c4e38a57531208f3bb623be65c20f0634d2ff7a073950cb4fa744da8da334d8','java',60,1795619162631.237305,1795619179953,4,NULL,0);
INSERT INTO files VALUES('src/java/cov/Dog.java','946e00b1ac95a2703f1579b144a36379f35061c8c96f4be07c8ce5e5986f8431','java',253,1795619162610.333496,1795619179953,7,NULL,0);
INSERT INTO files VALUES('src/java/cov/Pet.java','2c0db15b84866ff646046349a6dadec9a19bfda18299dc2026d4d07237db87f7','java',78,1795619162573.434083,1795619179953,4,NULL,0);
INSERT INTO files VALUES('src/js/main.js','108e3a236bdfafac703294fa9789660688d218bd6433dc0651ee1529ec8895cb','javascript',105,1795619162543.333496,1795619179954,2,NULL,0);
INSERT INTO files VALUES('src/js/sum.js','368b4edab62fcadb8e167c6dfd6ef852ebb2bd8d887759de6fdc7e202b955a5e','javascript',92,1795619162550.472168,1795619179954,2,NULL,0);
INSERT INTO files VALUES('src/kotlin/Zoo.kt','b80e2705909836f27a6efe2df850cf437bd37c2cb41149b6f0c5d5b9b52eb858','kotlin',313,1795619162667.731446,1795619179962,10,NULL,0);
INSERT INTO files VALUES('src/php/User.php','96fa804d860c0bbdc89f1c1686949aea47436dcf0c842b643e3cfe306ba8123c','php',450,1795619162700.123292,1795619179962,10,NULL,0);
INSERT INTO files VALUES('src/scala/Shop.scala','9d8edd75c6be3c53811c242d904174db2f4d0e9ed936be60c5d4e07d43dcee28','scala',298,1795619162807.15039,1795619179968,11,NULL,0);
INSERT INTO files VALUES('src/swift/Garden.swift','d610fb68d253d9d674588f7645e6282471b82ef1300fbc1aa0fb606eec0c50bf','swift',339,1795619162861.794434,1795619179973,9,NULL,0);
INSERT INTO files VALUES('src/ts/base.ts','1862a788f7332a290184a01d162bb48838c2e19dfc626f8feedd018ebbcd4166','typescript',211,1795619162961.421386,1795619179973,7,NULL,0);
INSERT INTO files VALUES('src/ts/nav.tsx','16c3ba1d77762ced5e8641b469d5fbbf1f124835f260f3d36378931be70e8a0d','tsx',296,1795619162952.671631,1795619179978,6,NULL,0);
INSERT INTO files VALUES('src/ts/shapes.ts','dd6fea27c34989d227b61afe63d2749a3ec27eba5cf4e91829de306ccd69b5a7','typescript',436,1795619163015.150146,1795619179978,10,NULL,0);
INSERT INTO files VALUES('src/vb/Counter.vb','55dc863dfcf9b14ce6e36ff684afad4e4f22aa47bb34f06d6a3116064b922710','vbnet',530,1795619162768.967773,1795619179984,9,NULL,0);
INSERT INTO nodes VALUES('file:src/c/util.c','file','util.c','src/c/util.c','src/c/util.c','c',1,9,0,0,NULL,NULL,NULL,0,0,0,0,NULL,NULL,NULL,1795619179940);
INSERT INTO nodes VALUES('import:4e3f53b5c88480f1d60f97f7908c1351','import','util.h','util.h','src/c/util.c','c',1,2,0,0,NULL,'#include "util.h"',NULL,0,0,0,0,NULL,NULL,NULL,1795619179940);
INSERT INTO nodes VALUES('function:280d59970ca0ad3ff2b14b2644077b24','function','add','add','src/c/util.c','c',3,3,0,39,NULL,NULL,NULL,0,0,0,0,NULL,NULL,NULL,1795619179940);
INSERT INTO nodes VALUES('function:fc3ca245cad8b54478051669881861b1','function','origin','origin','src/c/util.c','c',5,8,0,1,NULL,NULL,NULL,0,0,0,0,NULL,NULL,'point',1795619179940);
INSERT INTO nodes VALUES('file:src/c/util.h','file','util.h','src/c/util.h','src/c/util.h','c',1,7,0,0,NULL,NULL,NULL,0,0,0,0,NULL,NULL,NULL,1795619179941);
INSERT INTO nodes VALUES('struct:3f181e2cb864a47a39dedc4e66e9705c','struct','point','point','src/c/util.h','c',3,3,0,30,NULL,NULL,NULL,0,0,0,0,NULL,NULL,NULL,1795619179941);
INSERT INTO nodes VALUES('file:src/cpp/shape.cpp','file','shape.cpp','src/cpp/shape.cpp','src/cpp/shape.cpp','cpp',1,22,0,0,NULL,NULL,NULL,0,0,0,0,NULL,NULL,NULL,1795619179948);
INSERT INTO nodes VALUES('class:9534a8d8e20805f749522edcdb280079','class','Shape','geo::Shape','src/cpp/shape.cpp','cpp',3,7,0,1,NULL,NULL,NULL,0,0,0,0,NULL,NULL,NULL,1795619179948);
INSERT INTO nodes VALUES('method:0b29341d41062c5ba1711832ca02283a','method','area','geo::Shape::area','src/cpp/shape.cpp','cpp',5,5,4,45,NULL,NULL,'public',0,0,0,0,NULL,NULL,NULL,1795619179948);
INSERT INTO nodes VALUES('method:e3f2e3a8b01787ba81c843ed19d561b4','method','~Shape','geo::Shape::~Shape','src/cpp/shape.cpp','cpp',6,6,4,23,NULL,'()','public',0,0,0,0,NULL,NULL,NULL,1795619179948);
INSERT INTO nodes VALUES('class:6b1a4a49ca763fe0e3cba256ed8a25b7','class','Circle','geo::Circle','src/cpp/shape.cpp','cpp',9,13,0,1,NULL,NULL,NULL,0,0,0,0,NULL,NULL,NULL,1795619179948);
INSERT INTO nodes VALUES('method:065074d25565207d1637027918aacc80','method','area','geo::Circle::area','src/cpp/shape.cpp','cpp',12,12,4,57,NULL,NULL,'public',0,0,0,0,NULL,NULL,NULL,1795619179948);
INSERT INTO nodes VALUES('function:645161dc82eba1a211da31f011791d88','function','make_circle','geo::make_circle','src/cpp/shape.cpp','cpp',15,19,0,1,NULL,NULL,NULL,0,0,0,0,NULL,NULL,'Circle',1795619179948);
INSERT INTO nodes VALUES('file:src/csharp/Ledger.cs','file','Ledger.cs','src/csharp/Ledger.cs','src/csharp/Ledger.cs','csharp',1,24,0,0,NULL,NULL,NULL,0,0,0,0,NULL,NULL,NULL,1795619179953);
INSERT INTO nodes VALUES('namespace:5ea74a66a237826f93c5c0eaea681c79','namespace','Cov','Cov','src/csharp/Ledger.cs','csharp',1,23,0,1,NULL,NULL,NULL,0,0,0,0,NULL,NULL,NULL,1795619179953);
INSERT INTO nodes VALUES('interface:e5989f3d2fd52b469d7673ab2a5c2b76','interface','IEntry','Cov::IEntry','src/csharp/Ledger.cs','csharp',3,6,4,5,NULL,NULL,NULL,0,0,0,0,NULL,NULL,NULL,1795619179953);
INSERT INTO nodes VALUES('method:2f7e6964ed3a09c4c976eed4d53b3d96','method','Amount','Cov::IEntry::Amount','src/csharp/Ledger.cs','csharp',5,5,8,25,NULL,NULL,'public',0,0,0,0,NULL,NULL,NULL,1795619179953);
INSERT INTO nodes VALUES('class:973b84ac8bc455624f34d3456d50d81e','class','Money','Cov::Money','src/csharp/Ledger.cs','csharp',8,8,4,39,NULL,NULL,'public',0,0,0,0,NULL,NULL,NULL,1795619179953);
INSERT INTO nodes VALUES('class:73fe661196a2750c812ce51186aae73d','class','Entry','Cov::Entry','src/csharp/Ledger.cs','csharp',10,14,4,5,NULL,NULL,'public',0,0,0,0,NULL,NULL,NULL,1795619179953);
INSERT INTO nodes VALUES('method:0d10434ed95791a0ba1d9aa92771e570','method','Amount','Cov::Entry::Amount','src/csharp/Ledger.cs','csharp',12,12,8,41,NULL,NULL,'public',0,0,0,0,NULL,NULL,NULL,1795619179953);
INSERT INTO nodes VALUES('method:47506583e4f29dc8b70490b4b8622ff7','method','Label','Cov::Entry::Label','src/csharp/Ledger.cs','csharp',13,13,8,57,NULL,NULL,'public',0,0,0,0,NULL,NULL,NULL,1795619179953);
INSERT INTO nodes VALUES('class:75d27b4e0c431d399ecf3890b6965466','class','Credit','Cov::Credit','src/csharp/Ledger.cs','csharp',16,22,4,5,NULL,NULL,'public',0,0,0,0,NULL,NULL,NULL,1795619179953);
INSERT INTO nodes VALUES('field:e7f4a18c503437662f11568367446895','field','held','Cov::Credit::held','src/csharp/Ledger.cs','csharp',18,18,22,42,NULL,'Money held','private',0,0,0,0,NULL,NULL,NULL,1795619179953);
INSERT INTO nodes VALUES('method:49dcd1784a89f7c20feac7f59524718a','method','Amount','Cov::Credit::Amount','src/csharp/Ledger.cs','csharp',19,19,8,63,NULL,NULL,'public',0,0,0,0,NULL,NULL,NULL,1795619179953);
INSERT INTO nodes VALUES('method:7db6566d130023056ed61e675e44743c','method','Label','Cov::Credit::Label','src/csharp/Ledger.cs','csharp',20,20,8,59,NULL,NULL,'public',0,0,0,0,NULL,NULL,NULL,1795619179953);
INSERT INTO nodes VALUES('method:bf7128987788fb8c1e78ac3d5edfbca0','method','Open','Cov::Credit::Open','src/csharp/Ledger.cs','csharp',21,21,8,60,NULL,NULL,'public',0,0,1,0,NULL,NULL,'Credit',1795619179953);
INSERT INTO nodes VALUES('file:src/java/cov/Animal.java','file','Animal.java','src/java/cov/Animal.java','src/java/cov/Animal.java','java',1,6,0,0,NULL,NULL,NULL,0,0,0,0,NULL,NULL,NULL,1795619179953);
INSERT INTO nodes VALUES('namespace:351387a1db1ab22a305a5ed61dd0664a','namespace','cov','cov','src/java/cov/Animal.java','java',1,1,0,12,NULL,NULL,NULL,0,0,0,0,NULL,NULL,NULL,1795619179953);
INSERT INTO nodes VALUES('interface:160d3492ea8d26db30b32582af18b63d','interface','Animal','cov::Animal','src/java/cov/Animal.java','java',3,5,0,1,NULL,NULL,NULL,0,0,0,0,NULL,NULL,NULL,1795619179953);
INSERT INTO nodes VALUES('method:71540100da33120ac5619b792a756442','method','sound','cov::Animal::sound','src/java/cov/Animal.java','java',4,4,2,17,NULL,'String ()',NULL,0,0,0,0,NULL,NULL,'String',1795619179953);
INSERT INTO nodes VALUES('file:src/java/cov/Dog.java','file','Dog.java','src/java/cov/Dog.java','src/java/cov/Dog.java','java',1,19,0,0,NULL,NULL,NULL,0,0,0,0,NULL,NULL,NULL,1795619179953);
INSERT INTO nodes VALUES('namespace:6976c51302548418588337a7882fa6e5','namespace','cov','cov','src/java/cov/Dog.java','java',1,1,0,12,NULL,NULL,NULL,0,0,0,0,NULL,NULL,NULL,1795619179953);
INSERT INTO nodes VALUES('class:c34049fea971ed1009f6340858d5b6da','class','Dog','cov::Dog','src/java/cov/Dog.java','java',3,18,0,1,NULL,NULL,'public',0,0,0,0,NULL,NULL,NULL,1795619179953);
INSERT INTO nodes VALUES('field:afef0653456d9967a7a80c1159bd281e','field','friend','cov::Dog::friend','src/java/cov/Dog.java','java',4,4,14,20,NULL,'Pet friend','private',0,0,0,0,NULL,NULL,NULL,1795619179953);
INSERT INTO nodes VALUES('method:c529163609a31e0a9ea62be99d921af8','method','sound','cov::Dog::sound','src/java/cov/Dog.java','java',6,9,2,3,NULL,'String ()','public',0,0,0,0,NULL,NULL,'String',1795619179953);
INSERT INTO nodes VALUES('method:b8eb48518e6eaa5ac5e651c5792490a0','method','friend','cov::Dog::friend','src/java/cov/Dog.java','java',11,13,2,3,NULL,'Pet ()','public',0,0,0,0,NULL,NULL,'Pet',1795619179953);
INSERT INTO nodes VALUES('method:a37cab03c578042e7760a38e24a2d8ac','method','make','cov::Dog::make','src/java/cov/Dog.java','java',15,17,2,3,NULL,'Dog ()','public',0,0,1,0,NULL,NULL,'Dog',1795619179953);
INSERT INTO nodes VALUES('file:src/java/cov/Pet.java','file','Pet.java','src/java/cov/Pet.java','src/java/cov/Pet.java','java',1,6,0,0,NULL,NULL,NULL,0,0,0,0,NULL,NULL,NULL,1795619179954);
INSERT INTO nodes VALUES('namespace:cb234b82e0c2d4e26786c64fa35aaf38','namespace','cov','cov','src/java/cov/Pet.java','java',1,1,0,12,NULL,NULL,NULL,0,0,0,0,NULL,NULL,NULL,1795619179954);
INSERT INTO nodes VALUES('class:fd8bfa074e5a18adace881c69604bc22','class','Pet','cov::Pet','src/java/cov/Pet.java','java',3,5,0,1,NULL,NULL,'public',0,0,0,0,NULL,NULL,NULL,1795619179954);
INSERT INTO nodes VALUES('method:8bc91d238fa70bd653aa0315414fcbfe','method','sound','cov::Pet::sound','src/java/cov/Pet.java','java',4,4,2,33,NULL,'String ()','public',0,0,0,0,NULL,NULL,'String',1795619179954);
INSERT INTO nodes VALUES('file:src/js/main.js','file','main.js','src/js/main.js','src/js/main.js','javascript',1,8,0,0,NULL,NULL,NULL,0,0,0,0,NULL,NULL,NULL,1795619179953);
INSERT INTO nodes VALUES('function:92a69299d8ea63e456f0b538dc6b19f8','function','run','run','src/js/main.js','javascript',3,5,0,1,NULL,'(xs)',NULL,0,0,0,0,NULL,NULL,NULL,1795619179953);
INSERT INTO nodes VALUES('file:src/js/sum.js','file','sum.js','src/js/sum.js','src/js/sum.js','javascript',1,6,0,0,NULL,NULL,NULL,0,0,0,0,NULL,NULL,NULL,1795619179953);
INSERT INTO nodes VALUES('function:abca4f679782194f5335e769ff3b22de','function','total','total','src/js/sum.js','javascript',1,3,0,1,NULL,'(xs)',NULL,0,0,0,0,NULL,NULL,NULL,1795619179953);
INSERT INTO nodes VALUES('file:src/kotlin/Zoo.kt','file','Zoo.kt','src/kotlin/Zoo.kt','src/kotlin/Zoo.kt','kotlin',1,21,0,0,NULL,NULL,NULL,0,0,0,0,NULL,NULL,NULL,1795619179962);
INSERT INTO nodes VALUES('namespace:eaa6d890b9a7d024ebd30e4e9226959e','namespace','zoo','zoo','src/kotlin/Zoo.kt','kotlin',1,1,0,11,NULL,NULL,NULL,0,0,0,0,NULL,NULL,NULL,1795619179962);
INSERT INTO nodes VALUES('interface:8c9e6ae2d595a3843b2de40461d8bb84','interface','Feeder','zoo::Feeder','src/kotlin/Zoo.kt','kotlin',3,5,0,1,NULL,NULL,NULL,0,0,0,0,NULL,NULL,NULL,1795619179962);
INSERT INTO nodes VALUES('method:25e017c365b01ebf4862d1a827a54251','method','feed','zoo::Feeder::feed','src/kotlin/Zoo.kt','kotlin',4,4,4,25,NULL,'(n: Int): Int','public',0,0,0,0,NULL,NULL,'Int',1795619179962);
INSERT INTO nodes VALUES('class:30324f06309a6610fe9cecbb122fd5e1','class','Keeper','zoo::Keeper','src/kotlin/Zoo.kt','kotlin',7,9,0,1,NULL,NULL,'public',0,0,0,0,NULL,NULL,NULL,1795619179962);
INSERT INTO nodes VALUES('method:691c2a4590189fc8bcb5b655238b7e96','method','shift','zoo::Keeper::shift','src/kotlin/Zoo.kt','kotlin',8,8,4,36,NULL,'(): String','public',0,0,0,0,NULL,NULL,'String',1795619179962);
INSERT INTO nodes VALUES('class:4016dcf2b19c86a294df546264b8e14e','class','Night','zoo::Night','src/kotlin/Zoo.kt','kotlin',11,14,0,1,NULL,NULL,'public',0,0,0,0,NULL,NULL,NULL,1795619179962);
INSERT INTO nodes VALUES('method:440227f1abac63deb1c24eaaf0d55cce','method','shift','zoo::Night::shift','src/kotlin/Zoo.kt','kotlin',12,12,4,42,NULL,'(): String','public',0,0,0,0,NULL,NULL,'String',1795619179962);
INSERT INTO nodes VALUES('method:d7a05ad04e58dc2672f4eac0fbe9ed40','method','feed','zoo::Night::feed','src/kotlin/Zoo.kt','kotlin',13,13,4,42,NULL,'(n: Int): Int','public',0,0,0,0,NULL,NULL,'Int',1795619179962);
INSERT INTO nodes VALUES('function:ccb2e5f298268a07ad0278737ac03b49','function','hire','zoo::hire','src/kotlin/Zoo.kt','kotlin',16,20,0,1,NULL,'(): Keeper','public',0,0,0,0,NULL,NULL,'Keeper',1795619179962);
INSERT INTO nodes VALUES('file:src/php/User.php','file','User.php','src/php/User.php','src/php/User.php','php',1,30,0,0,NULL,NULL,NULL,0,0,0,0,NULL,NULL,NULL,1795619179962);
INSERT INTO nodes VALUES('namespace:0deac1d01cdb38a85d69bbee3a4e2387','namespace','Cov','Cov','src/php/User.php','php',2,2,0,14,NULL,NULL,NULL,0,0,0,0,NULL,NULL,NULL,1795619179962);
INSERT INTO nodes VALUES('interface:2ee5c9ee934832c9427b42287c8a274d','interface','HasName','Cov::HasName','src/php/User.php','php',4,7,0,1,NULL,NULL,NULL,0,0,0,0,NULL,NULL,NULL,1795619179962);
INSERT INTO nodes VALUES('method:b22378e1380a0857a869f6d5c014fe40','method','name','Cov::HasName::name','src/php/User.php','php',6,6,4,35,NULL,NULL,'public',0,0,0,0,NULL,NULL,NULL,1795619179962);
INSERT INTO nodes VALUES('class:0d45eeff2f959a3142735d62e2dec8f3','class','Model','Cov::Model','src/php/User.php','php',9,12,0,1,NULL,NULL,'public',0,0,0,0,NULL,NULL,NULL,1795619179962);
INSERT INTO nodes VALUES('method:f2a49bf2650167517d7e34c4ac8b9cb1','method','save','Cov::Model::save','src/php/User.php','php',11,11,4,49,NULL,NULL,'public',0,0,0,0,NULL,NULL,NULL,1795619179962);
INSERT INTO nodes VALUES('class:993e58866475e53b193cc0667f33be4f','class','User','Cov::User','src/php/User.php','php',14,29,0,1,NULL,NULL,'public',0,0,0,0,NULL,NULL,NULL,1795619179962);
INSERT INTO nodes VALUES('method:e802f9c9019a771e8e99e4078b684fd2','method','name','Cov::User::name','src/php/User.php','php',16,16,4,50,NULL,NULL,'public',0,0,0,0,NULL,NULL,NULL,1795619179962);
INSERT INTO nodes VALUES('method:f18b984e40f18154f7f42f1cd9775a9b','method','save','Cov::User::save','src/php/User.php','php',18,21,4,5,NULL,NULL,'public',0,0,0,0,NULL,NULL,NULL,1795619179962);
INSERT INTO nodes VALUES('method:761a8bdc126d2fc7ee60333c25c73145','method','make','Cov::User::make','src/php/User.php','php',23,28,4,5,NULL,NULL,'public',0,0,1,0,NULL,NULL,'User',1795619179962);
INSERT INTO nodes VALUES('file:src/scala/Shop.scala','file','Shop.scala','src/scala/Shop.scala','src/scala/Shop.scala','scala',1,20,0,0,NULL,NULL,NULL,0,0,0,0,NULL,NULL,NULL,1795619179968);
INSERT INTO nodes VALUES('trait:782c4f9275806860f183359b7e3ba567','trait','Priced','Priced','src/scala/Shop.scala','scala',3,5,0,1,NULL,NULL,'public',0,0,0,0,NULL,NULL,NULL,1795619179968);
INSERT INTO nodes VALUES('method:574ec8ad016447ebecbfd30ebc261362','method','price','Priced::price','src/scala/Shop.scala','scala',4,4,2,18,NULL,'(): Int','public',0,0,0,0,NULL,NULL,'Int',1795619179968);
INSERT INTO nodes VALUES('class:2f3b5beb3727e28fab45e80d7fe89013','class','Item','Item','src/scala/Shop.scala','scala',7,9,0,1,NULL,NULL,'public',0,0,0,0,NULL,NULL,NULL,1795619179968);
INSERT INTO nodes VALUES('method:f7f328a1f5208bb49949f6250cfeab41','method','label','Item::label','src/scala/Shop.scala','scala',8,8,2,30,NULL,'(): String','public',0,0,0,0,NULL,NULL,'String',1795619179968);
INSERT INTO nodes VALUES('class:7d656c5e71395b3d0433befbba7e57c0','class','Book','Book','src/scala/Shop.scala','scala',11,14,0,1,NULL,NULL,'public',0,0,0,0,NULL,NULL,NULL,1795619179968);
INSERT INTO nodes VALUES('method:90e2d82a912af8896f5ee0a6a6f1a4d5','method','label','Book::label','src/scala/Shop.scala','scala',12,12,2,39,NULL,'(): String','public',0,0,0,0,NULL,NULL,'String',1795619179968);
INSERT INTO nodes VALUES('method:cf1a518b70d9ec242b95531ff6269a69','method','price','Book::price','src/scala/Shop.scala','scala',13,13,2,23,NULL,'(): Int','public',0,0,0,0,NULL,NULL,'Int',1795619179968);
INSERT INTO nodes VALUES('module:87641851f68482f7fbaaeebea0b88c66','module','Shop','Shop','src/scala/Shop.scala','scala',16,19,0,1,NULL,NULL,'public',0,0,0,0,NULL,NULL,NULL,1795619179968);
INSERT INTO nodes VALUES('method:aedd864cd63a3f1c5e173b7d7c46623b','method','stock','Shop::stock','src/scala/Shop.scala','scala',17,17,2,32,NULL,'(): Book','public',0,0,0,0,NULL,NULL,'Book',1795619179968);
INSERT INTO nodes VALUES('method:166ac73221705937a15130993c8e6f52','method','total','Shop::total','src/scala/Shop.scala','scala',18,18,2,36,NULL,'(): Int','public',0,0,0,0,NULL,NULL,'Int',1795619179968);
INSERT INTO nodes VALUES('file:src/swift/Garden.swift','file','Garden.swift','src/swift/Garden.swift','src/swift/Garden.swift','swift',1,21,0,0,NULL,NULL,NULL,0,0,0,0,NULL,NULL,NULL,1795619179973);
INSERT INTO nodes VALUES('interface:eec467b641458241b1b6119c5cf12e8e','interface','Grows','Grows','src/swift/Garden.swift','swift',1,3,0,1,NULL,NULL,NULL,0,0,0,0,NULL,NULL,NULL,1795619179973);
INSERT INTO nodes VALUES('class:37ab8dac3f2b82a85ad5082f35d6acd5','class','Plant','Plant','src/swift/Garden.swift','swift',5,7,0,1,NULL,NULL,'internal',0,0,0,0,NULL,NULL,NULL,1795619179973);
INSERT INTO nodes VALUES('method:b1f68b015dc4b0a2280e76311b850b5a','method','water','Plant::water','src/swift/Garden.swift','swift',6,6,4,45,NULL,NULL,'internal',0,0,0,0,NULL,NULL,'String',1795619179973);
INSERT INTO nodes VALUES('class:b729f43d91ee3035bcbacd93c8d74bec','class','Tree','Tree','src/swift/Garden.swift','swift',9,12,0,1,NULL,NULL,'internal',0,0,0,0,NULL,NULL,NULL,1795619179973);
INSERT INTO nodes VALUES('method:c45312e63d3e84d2c63e5fa116077088','method','water','Tree::water','src/swift/Garden.swift','swift',10,10,4,53,NULL,NULL,'internal',0,0,0,0,NULL,NULL,'String',1795619179973);
INSERT INTO nodes VALUES('method:4b5a22f42e9b880337a5afd28ad5dead','method','grow','Tree::grow','src/swift/Garden.swift','swift',11,11,4,35,NULL,NULL,'internal',0,0,0,0,NULL,NULL,'Int',1795619179973);
INSERT INTO nodes VALUES('class:15b976172ac210f5bd33729f20f6c7c2','class','Tree','Tree','src/swift/Garden.swift','swift',14,20,0,1,NULL,NULL,'internal',0,0,0,0,NULL,NULL,NULL,1795619179973);
INSERT INTO nodes VALUES('method:09d646edd9b9814285aa71f520533300','method','prune','Tree::prune','src/swift/Garden.swift','swift',15,19,4,5,NULL,NULL,'internal',0,0,0,0,NULL,NULL,'Tree',1795619179973);
INSERT INTO nodes VALUES('file:src/ts/base.ts','file','base.ts','src/ts/base.ts','src/ts/base.ts','typescript',1,13,0,0,NULL,NULL,NULL,0,0,0,0,NULL,NULL,NULL,1795619179972);
INSERT INTO nodes VALUES('interface:305f6c466ff29b5114ca14e82e994e2b','interface','Named','Named','src/ts/base.ts','typescript',1,3,7,1,NULL,NULL,NULL,1,0,0,0,NULL,NULL,NULL,1795619179972);
INSERT INTO nodes VALUES('method:a6d6036576e54ae5f666c046d7f47015','method','name','Named::name','src/ts/base.ts','typescript',2,2,2,16,NULL,'(): string',NULL,0,0,0,0,NULL,NULL,NULL,1795619179972);
INSERT INTO nodes VALUES('class:d8180b6be277972027b4eee1b34bc5b8','class','Base','Base','src/ts/base.ts','typescript',4,9,7,1,NULL,NULL,NULL,1,0,0,0,NULL,NULL,NULL,1795619179972);
INSERT INTO nodes VALUES('method:5d0943d27416c726fa67260a6090df20','method','area','Base::area','src/ts/base.ts','typescript',5,5,2,25,NULL,'(): number',NULL,0,0,0,0,NULL,NULL,NULL,1795619179972);
INSERT INTO nodes VALUES('method:8e03dc9b0aa566fad75eacd2a1aa88d3','method','describe','Base::describe','src/ts/base.ts','typescript',6,8,2,3,NULL,'(): string',NULL,0,0,0,0,NULL,NULL,NULL,1795619179972);
INSERT INTO nodes VALUES('function:18435f682dce2df791235756b0e21179','function','log','log','src/ts/base.ts','typescript',10,12,7,1,NULL,'(target: unknown): void',NULL,1,0,0,0,NULL,NULL,NULL,1795619179972);
INSERT INTO nodes VALUES('file:src/ts/nav.tsx','file','nav.tsx','src/ts/nav.tsx','src/ts/nav.tsx','tsx',1,18,0,0,NULL,NULL,NULL,0,0,0,0,NULL,NULL,NULL,1795619179976);
INSERT INTO nodes VALUES('import:d21db863cd7910573865c8b607bdc7c4','import','react-router-dom','react-router-dom','src/ts/nav.tsx','tsx',1,1,0,55,NULL,'import { Link, Route, Routes } from "react-router-dom";',NULL,0,0,0,0,NULL,NULL,NULL,1795619179976);
INSERT INTO nodes VALUES('function:c415287c16b83edcd291de6fa946b463','function','About','About','src/ts/nav.tsx','tsx',3,5,7,1,NULL,'()',NULL,1,0,0,0,NULL,NULL,NULL,1795619179976);
INSERT INTO nodes VALUES('function:a6a1f926e933634f309625d1748f96c5','function','Nav','Nav','src/ts/nav.tsx','tsx',7,9,7,1,NULL,'()',NULL,1,0,0,0,NULL,NULL,NULL,1795619179976);
INSERT INTO nodes VALUES('function:d1341d521c832b672652de06edc1eedc','function','App','App','src/ts/nav.tsx','tsx',11,17,7,1,NULL,'()',NULL,1,0,0,0,NULL,NULL,NULL,1795619179976);
INSERT INTO nodes VALUES('route:src/ts/nav.tsx:14:/about','route','/about','src/ts/nav.tsx::route:/about','src/ts/nav.tsx','tsx',14,14,0,0,NULL,NULL,NULL,0,0,0,0,NULL,NULL,NULL,1795619179976);
INSERT INTO nodes VALUES('file:src/ts/shapes.ts','file','shapes.ts','src/ts/shapes.ts','src/ts/shapes.ts','typescript',1,25,0,0,NULL,NULL,NULL,0,0,0,0,NULL,NULL,NULL,1795619179974);
INSERT INTO nodes VALUES('import:fbab2ecc5e8da169e370388f36933b92','import','./base','./base','src/ts/shapes.ts','typescript',1,1,0,42,NULL,'import { Base, Named, log } from "./base";',NULL,0,0,0,0,NULL,NULL,NULL,1795619179974);
INSERT INTO nodes VALUES('import:63a1ba20b9ef4025857f14a75adc0bd4','import','./theme.css','./theme.css','src/ts/shapes.ts','typescript',2,2,0,21,NULL,'import "./theme.css";',NULL,0,0,0,0,NULL,NULL,NULL,1795619179974);
INSERT INTO nodes VALUES('import:ac350905b85cc347000040ef1db508ba','import','./base','./base','src/ts/shapes.ts','typescript',4,4,0,29,NULL,'export { log } from "./base";',NULL,0,0,0,0,NULL,NULL,NULL,1795619179974);
INSERT INTO nodes VALUES('class:f74173ac01e5f0e1bed08f474b7ba73e','class','Square','Square','src/ts/shapes.ts','typescript',7,18,7,1,NULL,NULL,NULL,1,0,0,0,NULL,NULL,NULL,1795619179974);
INSERT INTO nodes VALUES('property:30940e3a62462c0c0fa824c3ff2c80a1','property','side','Square::side','src/ts/shapes.ts','typescript',8,8,2,18,NULL,'number side',NULL,0,0,0,0,NULL,NULL,NULL,1795619179974);
INSERT INTO nodes VALUES('method:18c44c380fe5bc0b19196abcf34e4130','method','area','Square::area','src/ts/shapes.ts','typescript',9,11,2,3,NULL,'(): number',NULL,0,0,0,0,NULL,NULL,NULL,1795619179974);
INSERT INTO nodes VALUES('method:1bf30bc0739c597f5c76278ff83be991','method','describe','Square::describe','src/ts/shapes.ts','typescript',12,14,2,3,NULL,'(): string',NULL,0,0,0,0,NULL,NULL,NULL,1795619179974);
INSERT INTO nodes VALUES('method:17d817ce728de8e0f867c7adac9f2b2e','method','name','Square::name','src/ts/shapes.ts','typescript',15,17,2,3,NULL,'(): string',NULL,0,0,0,0,NULL,NULL,NULL,1795619179974);
INSERT INTO nodes VALUES('function:f85442201d298699a22ac31b10aa022a','function','grow','grow','src/ts/shapes.ts','typescript',20,24,7,1,NULL,'(s: Square): Square',NULL,1,0,0,0,NULL,NULL,NULL,1795619179974);
INSERT INTO nodes VALUES('file:src/vb/Counter.vb','file','Counter.vb','src/vb/Counter.vb','src/vb/Counter.vb','vbnet',1,26,0,0,NULL,NULL,NULL,0,0,0,0,NULL,NULL,NULL,1795619179979);
INSERT INTO nodes VALUES('interface:b3000144199c43792a9145aa39609829','interface','ICounter','ICounter','src/vb/Counter.vb','vbnet',1,5,0,0,NULL,NULL,NULL,0,0,0,0,NULL,NULL,NULL,1795619179981);
INSERT INTO nodes VALUES('method:2ca3be137999b2aafb0f8dba69348828','method','Current','ICounter::Current','src/vb/Counter.vb','vbnet',2,3,4,0,NULL,NULL,'public',0,0,0,0,NULL,NULL,NULL,1795619179982);
INSERT INTO nodes VALUES('class:d65ac71869a56319926c526e9b1ec66f','class','Counter','Counter','src/vb/Counter.vb','vbnet',5,19,0,0,NULL,NULL,'public',0,0,0,0,NULL,NULL,NULL,1795619179982);
INSERT INTO nodes VALUES('field:e828cf90fe52cfa733e36195496bb6ea','field','items','Counter::items','src/vb/Counter.vb','vbnet',8,8,12,56,NULL,'Private items','private',0,0,0,0,NULL,NULL,NULL,1795619179983);
INSERT INTO nodes VALUES('method:496dabfc265607b18b2ae8f50e960ed1','method','Current','Counter::Current','src/vb/Counter.vb','vbnet',10,14,4,0,NULL,NULL,'public',0,0,0,0,NULL,NULL,NULL,1795619179983);
INSERT INTO nodes VALUES('method:060a1b20a3309a78778fb8495e0274cc','method','Name','Counter::Name','src/vb/Counter.vb','vbnet',14,17,4,0,NULL,NULL,'public',0,0,0,0,NULL,NULL,NULL,1795619179984);
INSERT INTO nodes VALUES('class:bfdaa2a96ce901cd41990eca37f989c0','class','Tally','Tally','src/vb/Counter.vb','vbnet',19,26,0,0,NULL,NULL,'public',0,0,0,0,NULL,NULL,NULL,1795619179984);
INSERT INTO nodes VALUES('method:761c485fc676053e1084043275ff017d','method','Name','Tally::Name','src/vb/Counter.vb','vbnet',22,25,4,0,NULL,NULL,'public',0,0,0,0,NULL,NULL,NULL,1795619179984);
INSERT INTO edges VALUES(1,'file:src/c/util.c','import:4e3f53b5c88480f1d60f97f7908c1351','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(2,'file:src/c/util.c','function:280d59970ca0ad3ff2b14b2644077b24','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(3,'file:src/c/util.c','function:fc3ca245cad8b54478051669881861b1','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(4,'file:src/c/util.h','struct:3f181e2cb864a47a39dedc4e66e9705c','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(5,'file:src/cpp/shape.cpp','class:9534a8d8e20805f749522edcdb280079','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(6,'class:9534a8d8e20805f749522edcdb280079','method:0b29341d41062c5ba1711832ca02283a','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(7,'class:9534a8d8e20805f749522edcdb280079','method:e3f2e3a8b01787ba81c843ed19d561b4','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(8,'file:src/cpp/shape.cpp','class:6b1a4a49ca763fe0e3cba256ed8a25b7','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(9,'class:6b1a4a49ca763fe0e3cba256ed8a25b7','method:065074d25565207d1637027918aacc80','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(10,'file:src/cpp/shape.cpp','function:645161dc82eba1a211da31f011791d88','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(11,'file:src/csharp/Ledger.cs','namespace:5ea74a66a237826f93c5c0eaea681c79','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(12,'namespace:5ea74a66a237826f93c5c0eaea681c79','interface:e5989f3d2fd52b469d7673ab2a5c2b76','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(13,'interface:e5989f3d2fd52b469d7673ab2a5c2b76','method:2f7e6964ed3a09c4c976eed4d53b3d96','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(14,'namespace:5ea74a66a237826f93c5c0eaea681c79','class:973b84ac8bc455624f34d3456d50d81e','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(15,'namespace:5ea74a66a237826f93c5c0eaea681c79','class:73fe661196a2750c812ce51186aae73d','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(16,'class:73fe661196a2750c812ce51186aae73d','method:0d10434ed95791a0ba1d9aa92771e570','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(17,'class:73fe661196a2750c812ce51186aae73d','method:47506583e4f29dc8b70490b4b8622ff7','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(18,'namespace:5ea74a66a237826f93c5c0eaea681c79','class:75d27b4e0c431d399ecf3890b6965466','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(19,'class:75d27b4e0c431d399ecf3890b6965466','field:e7f4a18c503437662f11568367446895','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(20,'class:75d27b4e0c431d399ecf3890b6965466','method:49dcd1784a89f7c20feac7f59524718a','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(21,'class:75d27b4e0c431d399ecf3890b6965466','method:7db6566d130023056ed61e675e44743c','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(22,'class:75d27b4e0c431d399ecf3890b6965466','method:bf7128987788fb8c1e78ac3d5edfbca0','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(23,'file:src/java/cov/Animal.java','namespace:351387a1db1ab22a305a5ed61dd0664a','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(24,'namespace:351387a1db1ab22a305a5ed61dd0664a','interface:160d3492ea8d26db30b32582af18b63d','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(25,'interface:160d3492ea8d26db30b32582af18b63d','method:71540100da33120ac5619b792a756442','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(26,'file:src/java/cov/Dog.java','namespace:6976c51302548418588337a7882fa6e5','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(27,'namespace:6976c51302548418588337a7882fa6e5','class:c34049fea971ed1009f6340858d5b6da','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(28,'class:c34049fea971ed1009f6340858d5b6da','field:afef0653456d9967a7a80c1159bd281e','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(29,'class:c34049fea971ed1009f6340858d5b6da','method:c529163609a31e0a9ea62be99d921af8','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(30,'class:c34049fea971ed1009f6340858d5b6da','method:b8eb48518e6eaa5ac5e651c5792490a0','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(31,'class:c34049fea971ed1009f6340858d5b6da','method:a37cab03c578042e7760a38e24a2d8ac','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(32,'file:src/java/cov/Pet.java','namespace:cb234b82e0c2d4e26786c64fa35aaf38','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(33,'namespace:cb234b82e0c2d4e26786c64fa35aaf38','class:fd8bfa074e5a18adace881c69604bc22','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(34,'class:fd8bfa074e5a18adace881c69604bc22','method:8bc91d238fa70bd653aa0315414fcbfe','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(35,'file:src/js/main.js','function:92a69299d8ea63e456f0b538dc6b19f8','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(36,'file:src/js/sum.js','function:abca4f679782194f5335e769ff3b22de','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(37,'file:src/kotlin/Zoo.kt','namespace:eaa6d890b9a7d024ebd30e4e9226959e','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(38,'namespace:eaa6d890b9a7d024ebd30e4e9226959e','interface:8c9e6ae2d595a3843b2de40461d8bb84','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(39,'interface:8c9e6ae2d595a3843b2de40461d8bb84','method:25e017c365b01ebf4862d1a827a54251','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(40,'namespace:eaa6d890b9a7d024ebd30e4e9226959e','class:30324f06309a6610fe9cecbb122fd5e1','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(41,'class:30324f06309a6610fe9cecbb122fd5e1','method:691c2a4590189fc8bcb5b655238b7e96','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(42,'namespace:eaa6d890b9a7d024ebd30e4e9226959e','class:4016dcf2b19c86a294df546264b8e14e','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(43,'class:4016dcf2b19c86a294df546264b8e14e','method:440227f1abac63deb1c24eaaf0d55cce','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(44,'class:4016dcf2b19c86a294df546264b8e14e','method:d7a05ad04e58dc2672f4eac0fbe9ed40','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(45,'namespace:eaa6d890b9a7d024ebd30e4e9226959e','function:ccb2e5f298268a07ad0278737ac03b49','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(46,'file:src/php/User.php','namespace:0deac1d01cdb38a85d69bbee3a4e2387','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(47,'namespace:0deac1d01cdb38a85d69bbee3a4e2387','interface:2ee5c9ee934832c9427b42287c8a274d','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(48,'interface:2ee5c9ee934832c9427b42287c8a274d','method:b22378e1380a0857a869f6d5c014fe40','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(49,'namespace:0deac1d01cdb38a85d69bbee3a4e2387','class:0d45eeff2f959a3142735d62e2dec8f3','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(50,'class:0d45eeff2f959a3142735d62e2dec8f3','method:f2a49bf2650167517d7e34c4ac8b9cb1','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(51,'namespace:0deac1d01cdb38a85d69bbee3a4e2387','class:993e58866475e53b193cc0667f33be4f','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(52,'class:993e58866475e53b193cc0667f33be4f','method:e802f9c9019a771e8e99e4078b684fd2','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(53,'class:993e58866475e53b193cc0667f33be4f','method:f18b984e40f18154f7f42f1cd9775a9b','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(54,'class:993e58866475e53b193cc0667f33be4f','method:761a8bdc126d2fc7ee60333c25c73145','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(55,'file:src/scala/Shop.scala','trait:782c4f9275806860f183359b7e3ba567','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(56,'trait:782c4f9275806860f183359b7e3ba567','method:574ec8ad016447ebecbfd30ebc261362','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(57,'file:src/scala/Shop.scala','class:2f3b5beb3727e28fab45e80d7fe89013','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(58,'class:2f3b5beb3727e28fab45e80d7fe89013','method:f7f328a1f5208bb49949f6250cfeab41','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(59,'file:src/scala/Shop.scala','class:7d656c5e71395b3d0433befbba7e57c0','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(60,'class:7d656c5e71395b3d0433befbba7e57c0','method:90e2d82a912af8896f5ee0a6a6f1a4d5','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(61,'class:7d656c5e71395b3d0433befbba7e57c0','method:cf1a518b70d9ec242b95531ff6269a69','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(62,'file:src/scala/Shop.scala','module:87641851f68482f7fbaaeebea0b88c66','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(63,'module:87641851f68482f7fbaaeebea0b88c66','method:aedd864cd63a3f1c5e173b7d7c46623b','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(64,'module:87641851f68482f7fbaaeebea0b88c66','method:166ac73221705937a15130993c8e6f52','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(65,'file:src/swift/Garden.swift','interface:eec467b641458241b1b6119c5cf12e8e','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(66,'file:src/swift/Garden.swift','class:37ab8dac3f2b82a85ad5082f35d6acd5','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(67,'class:37ab8dac3f2b82a85ad5082f35d6acd5','method:b1f68b015dc4b0a2280e76311b850b5a','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(68,'file:src/swift/Garden.swift','class:b729f43d91ee3035bcbacd93c8d74bec','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(69,'class:b729f43d91ee3035bcbacd93c8d74bec','method:c45312e63d3e84d2c63e5fa116077088','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(70,'class:b729f43d91ee3035bcbacd93c8d74bec','method:4b5a22f42e9b880337a5afd28ad5dead','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(71,'file:src/swift/Garden.swift','class:15b976172ac210f5bd33729f20f6c7c2','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(72,'class:15b976172ac210f5bd33729f20f6c7c2','method:09d646edd9b9814285aa71f520533300','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(73,'file:src/ts/base.ts','interface:305f6c466ff29b5114ca14e82e994e2b','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(74,'interface:305f6c466ff29b5114ca14e82e994e2b','method:a6d6036576e54ae5f666c046d7f47015','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(75,'file:src/ts/base.ts','class:d8180b6be277972027b4eee1b34bc5b8','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(76,'class:d8180b6be277972027b4eee1b34bc5b8','method:5d0943d27416c726fa67260a6090df20','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(77,'class:d8180b6be277972027b4eee1b34bc5b8','method:8e03dc9b0aa566fad75eacd2a1aa88d3','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(78,'file:src/ts/base.ts','function:18435f682dce2df791235756b0e21179','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(79,'file:src/ts/nav.tsx','import:d21db863cd7910573865c8b607bdc7c4','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(80,'file:src/ts/nav.tsx','function:c415287c16b83edcd291de6fa946b463','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(81,'file:src/ts/nav.tsx','function:a6a1f926e933634f309625d1748f96c5','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(82,'file:src/ts/nav.tsx','function:d1341d521c832b672652de06edc1eedc','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(83,'file:src/ts/shapes.ts','import:fbab2ecc5e8da169e370388f36933b92','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(84,'file:src/ts/shapes.ts','import:63a1ba20b9ef4025857f14a75adc0bd4','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(85,'file:src/ts/shapes.ts','import:ac350905b85cc347000040ef1db508ba','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(86,'file:src/ts/shapes.ts','class:f74173ac01e5f0e1bed08f474b7ba73e','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(87,'class:f74173ac01e5f0e1bed08f474b7ba73e','property:30940e3a62462c0c0fa824c3ff2c80a1','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(88,'class:f74173ac01e5f0e1bed08f474b7ba73e','method:18c44c380fe5bc0b19196abcf34e4130','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(89,'class:f74173ac01e5f0e1bed08f474b7ba73e','method:1bf30bc0739c597f5c76278ff83be991','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(90,'class:f74173ac01e5f0e1bed08f474b7ba73e','method:17d817ce728de8e0f867c7adac9f2b2e','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(91,'file:src/ts/shapes.ts','function:f85442201d298699a22ac31b10aa022a','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(92,'file:src/vb/Counter.vb','interface:b3000144199c43792a9145aa39609829','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(93,'interface:b3000144199c43792a9145aa39609829','method:2ca3be137999b2aafb0f8dba69348828','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(94,'file:src/vb/Counter.vb','class:d65ac71869a56319926c526e9b1ec66f','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(95,'class:d65ac71869a56319926c526e9b1ec66f','field:e828cf90fe52cfa733e36195496bb6ea','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(96,'class:d65ac71869a56319926c526e9b1ec66f','method:496dabfc265607b18b2ae8f50e960ed1','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(97,'class:d65ac71869a56319926c526e9b1ec66f','method:060a1b20a3309a78778fb8495e0274cc','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(98,'file:src/vb/Counter.vb','class:bfdaa2a96ce901cd41990eca37f989c0','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(99,'class:bfdaa2a96ce901cd41990eca37f989c0','method:761c485fc676053e1084043275ff017d','contains',NULL,NULL,NULL,NULL);
INSERT INTO edges VALUES(100,'file:src/c/util.c','file:src/c/util.h','imports','{"confidence":0.92,"resolvedBy":"import","refName":"util.h"}',1,0,NULL);
INSERT INTO edges VALUES(101,'class:6b1a4a49ca763fe0e3cba256ed8a25b7','class:9534a8d8e20805f749522edcdb280079','extends','{"confidence":0.9,"resolvedBy":"qualified-name","refName":"Shape"}',9,22,NULL);
INSERT INTO edges VALUES(102,'class:73fe661196a2750c812ce51186aae73d','interface:e5989f3d2fd52b469d7673ab2a5c2b76','implements','{"confidence":0.9,"resolvedBy":"exact-match","refName":"IEntry","refKind":"extends"}',10,34,NULL);
INSERT INTO edges VALUES(103,'class:75d27b4e0c431d399ecf3890b6965466','class:73fe661196a2750c812ce51186aae73d','extends','{"confidence":0.9,"resolvedBy":"exact-match","refName":"Entry"}',16,26,NULL);
INSERT INTO edges VALUES(104,'class:c34049fea971ed1009f6340858d5b6da','class:fd8bfa074e5a18adace881c69604bc22','extends','{"confidence":0.9,"resolvedBy":"exact-match","refName":"Pet"}',3,25,NULL);
INSERT INTO edges VALUES(105,'class:c34049fea971ed1009f6340858d5b6da','interface:160d3492ea8d26db30b32582af18b63d','implements','{"confidence":0.9,"resolvedBy":"exact-match","refName":"Animal"}',3,40,NULL);
INSERT INTO edges VALUES(106,'file:src/js/main.js','file:src/js/sum.js','imports','{"confidence":0.9,"resolvedBy":"import","refName":"./sum"}',1,18,NULL);
INSERT INTO edges VALUES(107,'class:4016dcf2b19c86a294df546264b8e14e','class:30324f06309a6610fe9cecbb122fd5e1','extends','{"confidence":0.9,"resolvedBy":"exact-match","refName":"Keeper"}',11,14,NULL);
INSERT INTO edges VALUES(108,'class:4016dcf2b19c86a294df546264b8e14e','interface:8c9e6ae2d595a3843b2de40461d8bb84','implements','{"confidence":0.9,"resolvedBy":"exact-match","refName":"Feeder","refKind":"extends"}',11,24,NULL);
INSERT INTO edges VALUES(109,'class:993e58866475e53b193cc0667f33be4f','class:0d45eeff2f959a3142735d62e2dec8f3','extends','{"confidence":0.9,"resolvedBy":"exact-match","refName":"Model"}',14,19,NULL);
INSERT INTO edges VALUES(110,'class:993e58866475e53b193cc0667f33be4f','interface:2ee5c9ee934832c9427b42287c8a274d','implements','{"confidence":0.9,"resolvedBy":"exact-match","refName":"HasName"}',14,36,NULL);
INSERT INTO edges VALUES(111,'class:7d656c5e71395b3d0433befbba7e57c0','class:2f3b5beb3727e28fab45e80d7fe89013','extends','{"confidence":0.9,"resolvedBy":"exact-match","refName":"Item"}',11,19,NULL);
INSERT INTO edges VALUES(112,'class:7d656c5e71395b3d0433befbba7e57c0','trait:782c4f9275806860f183359b7e3ba567','extends','{"confidence":0.9,"resolvedBy":"exact-match","refName":"Priced"}',11,29,NULL);
INSERT INTO edges VALUES(113,'class:b729f43d91ee3035bcbacd93c8d74bec','class:37ab8dac3f2b82a85ad5082f35d6acd5','extends','{"confidence":0.9,"resolvedBy":"exact-match","refName":"Plant"}',9,12,NULL);
INSERT INTO edges VALUES(114,'class:b729f43d91ee3035bcbacd93c8d74bec','interface:eec467b641458241b1b6119c5cf12e8e','implements','{"confidence":0.9,"resolvedBy":"exact-match","refName":"Grows","refKind":"extends"}',9,19,NULL);
INSERT INTO edges VALUES(115,'file:src/ts/shapes.ts','file:src/ts/base.ts','imports','{"confidence":0.9,"resolvedBy":"import","refName":"./base"}',1,0,NULL);
INSERT INTO edges VALUES(116,'file:src/ts/shapes.ts','class:d8180b6be277972027b4eee1b34bc5b8','imports','{"confidence":0.9,"resolvedBy":"import","refName":"Base"}',1,9,NULL);
INSERT INTO edges VALUES(117,'file:src/ts/shapes.ts','interface:305f6c466ff29b5114ca14e82e994e2b','imports','{"confidence":0.9,"resolvedBy":"import","refName":"Named"}',1,15,NULL);
INSERT INTO edges VALUES(118,'file:src/ts/shapes.ts','function:18435f682dce2df791235756b0e21179','imports','{"confidence":0.9,"resolvedBy":"import","refName":"log"}',1,22,NULL);
INSERT INTO edges VALUES(119,'file:src/ts/shapes.ts','file:src/ts/base.ts','imports','{"confidence":0.9,"resolvedBy":"import","refName":"./base"}',4,0,NULL);
INSERT INTO edges VALUES(120,'file:src/ts/shapes.ts','function:18435f682dce2df791235756b0e21179','imports','{"confidence":0.9,"resolvedBy":"import","refName":"log"}',4,9,NULL);
INSERT INTO edges VALUES(121,'class:f74173ac01e5f0e1bed08f474b7ba73e','class:d8180b6be277972027b4eee1b34bc5b8','extends','{"confidence":0.9,"resolvedBy":"import","refName":"Base"}',7,28,NULL);
INSERT INTO edges VALUES(122,'class:f74173ac01e5f0e1bed08f474b7ba73e','interface:305f6c466ff29b5114ca14e82e994e2b','implements','{"confidence":0.9,"resolvedBy":"import","refName":"Named"}',7,44,NULL);
INSERT INTO edges VALUES(123,'class:d65ac71869a56319926c526e9b1ec66f','interface:b3000144199c43792a9145aa39609829','implements','{"confidence":0.9,"resolvedBy":"exact-match","refName":"ICounter"}',6,15,NULL);
INSERT INTO edges VALUES(124,'class:bfdaa2a96ce901cd41990eca37f989c0','class:d65ac71869a56319926c526e9b1ec66f','extends','{"confidence":0.9,"resolvedBy":"exact-match","refName":"Counter"}',20,13,NULL);
INSERT INTO edges VALUES(125,'function:fc3ca245cad8b54478051669881861b1','function:280d59970ca0ad3ff2b14b2644077b24','calls','{"confidence":0.9,"resolvedBy":"exact-match","refName":"add"}',6,23,NULL);
INSERT INTO edges VALUES(126,'function:645161dc82eba1a211da31f011791d88','method:065074d25565207d1637027918aacc80','calls','{"confidence":0.9,"resolvedBy":"instance-method","refName":"c.area"}',17,4,NULL);
INSERT INTO edges VALUES(127,'field:e7f4a18c503437662f11568367446895','class:973b84ac8bc455624f34d3456d50d81e','references','{"confidence":0.9,"resolvedBy":"exact-match","refName":"Money"}',18,16,NULL);
INSERT INTO edges VALUES(128,'field:e7f4a18c503437662f11568367446895','class:973b84ac8bc455624f34d3456d50d81e','instantiates','{"confidence":0.9,"resolvedBy":"exact-match","refName":"Money"}',18,29,NULL);
INSERT INTO edges VALUES(129,'method:bf7128987788fb8c1e78ac3d5edfbca0','class:75d27b4e0c431d399ecf3890b6965466','references','{"confidence":0.9,"resolvedBy":"exact-match","refName":"Credit"}',21,22,NULL);
INSERT INTO edges VALUES(130,'method:bf7128987788fb8c1e78ac3d5edfbca0','class:75d27b4e0c431d399ecf3890b6965466','instantiates','{"confidence":0.9,"resolvedBy":"exact-match","refName":"Credit"}',21,45,NULL);
INSERT INTO edges VALUES(131,'field:afef0653456d9967a7a80c1159bd281e','class:fd8bfa074e5a18adace881c69604bc22','references','{"confidence":0.9,"resolvedBy":"exact-match","refName":"Pet"}',4,10,NULL);
INSERT INTO edges VALUES(132,'method:b8eb48518e6eaa5ac5e651c5792490a0','class:fd8bfa074e5a18adace881c69604bc22','references','{"confidence":0.9,"resolvedBy":"exact-match","refName":"Pet"}',11,9,NULL);
INSERT INTO edges VALUES(133,'method:a37cab03c578042e7760a38e24a2d8ac','class:c34049fea971ed1009f6340858d5b6da','references','{"confidence":0.9,"resolvedBy":"exact-match","refName":"Dog"}',15,16,NULL);
INSERT INTO edges VALUES(134,'method:a37cab03c578042e7760a38e24a2d8ac','class:c34049fea971ed1009f6340858d5b6da','instantiates','{"confidence":0.9,"resolvedBy":"exact-match","refName":"Dog"}',16,11,NULL);
INSERT INTO edges VALUES(135,'function:92a69299d8ea63e456f0b538dc6b19f8','function:abca4f679782194f5335e769ff3b22de','calls','{"confidence":0.9,"resolvedBy":"import","refName":"total"}',4,9,NULL);
INSERT INTO edges VALUES(136,'file:src/js/main.js','function:92a69299d8ea63e456f0b538dc6b19f8','references','{"confidence":0.95,"resolvedBy":"function-ref","refName":"run","refKind":"function_ref","fnRef":true}',7,19,NULL);
INSERT INTO edges VALUES(137,'file:src/js/sum.js','function:abca4f679782194f5335e769ff3b22de','references','{"confidence":0.95,"resolvedBy":"function-ref","refName":"total","refKind":"function_ref","fnRef":true}',5,19,NULL);
INSERT INTO edges VALUES(138,'function:ccb2e5f298268a07ad0278737ac03b49','class:4016dcf2b19c86a294df546264b8e14e','instantiates','{"confidence":0.9,"resolvedBy":"exact-match","refName":"Night","refKind":"calls"}',17,12,NULL);
INSERT INTO edges VALUES(139,'function:ccb2e5f298268a07ad0278737ac03b49','method:d7a05ad04e58dc2672f4eac0fbe9ed40','calls','{"confidence":0.9,"resolvedBy":"instance-method","refName":"k.feed"}',18,4,NULL);
INSERT INTO edges VALUES(140,'method:f18b984e40f18154f7f42f1cd9775a9b','method:f2a49bf2650167517d7e34c4ac8b9cb1','calls','{"confidence":0.9,"resolvedBy":"exact-match","refName":"save"}',20,15,NULL);
INSERT INTO edges VALUES(141,'method:761a8bdc126d2fc7ee60333c25c73145','class:993e58866475e53b193cc0667f33be4f','references','{"confidence":0.9,"resolvedBy":"exact-match","refName":"User"}',23,35,NULL);
INSERT INTO edges VALUES(142,'method:761a8bdc126d2fc7ee60333c25c73145','class:993e58866475e53b193cc0667f33be4f','instantiates','{"confidence":0.9,"resolvedBy":"exact-match","refName":"User"}',25,13,NULL);
INSERT INTO edges VALUES(143,'method:761a8bdc126d2fc7ee60333c25c73145','method:f18b984e40f18154f7f42f1cd9775a9b','calls','{"confidence":0.9,"resolvedBy":"instance-method","refName":"u.save"}',26,8,NULL);
INSERT INTO edges VALUES(144,'method:aedd864cd63a3f1c5e173b7d7c46623b','class:7d656c5e71395b3d0433befbba7e57c0','references','{"confidence":0.9,"resolvedBy":"exact-match","refName":"Book"}',17,15,NULL);
INSERT INTO edges VALUES(145,'method:aedd864cd63a3f1c5e173b7d7c46623b','class:7d656c5e71395b3d0433befbba7e57c0','instantiates','{"confidence":0.9,"resolvedBy":"exact-match","refName":"Book"}',17,22,NULL);
INSERT INTO edges VALUES(146,'method:166ac73221705937a15130993c8e6f52','method:cf1a518b70d9ec242b95531ff6269a69','calls','{"confidence":0.7,"resolvedBy":"exact-match","refName":"price"}',18,21,NULL);
INSERT INTO edges VALUES(147,'method:166ac73221705937a15130993c8e6f52','method:aedd864cd63a3f1c5e173b7d7c46623b','calls','{"confidence":0.9,"resolvedBy":"exact-match","refName":"stock"}',18,21,NULL);
INSERT INTO edges VALUES(148,'method:09d646edd9b9814285aa71f520533300','class:b729f43d91ee3035bcbacd93c8d74bec','references','{"confidence":0.7,"resolvedBy":"exact-match","refName":"Tree"}',15,20,NULL);
INSERT INTO edges VALUES(149,'method:09d646edd9b9814285aa71f520533300','class:b729f43d91ee3035bcbacd93c8d74bec','instantiates','{"confidence":0.7,"resolvedBy":"exact-match","refName":"Tree","refKind":"calls"}',16,16,NULL);
INSERT INTO edges VALUES(150,'method:09d646edd9b9814285aa71f520533300','method:4b5a22f42e9b880337a5afd28ad5dead','calls','{"confidence":0.9,"resolvedBy":"instance-method","refName":"t.grow"}',17,12,NULL);
INSERT INTO edges VALUES(151,'route:src/ts/nav.tsx:14:/about','function:c415287c16b83edcd291de6fa946b463','references','{"confidence":0.9,"resolvedBy":"exact-match","refName":"About"}',14,0,NULL);
INSERT INTO edges VALUES(152,'class:f74173ac01e5f0e1bed08f474b7ba73e','function:18435f682dce2df791235756b0e21179','decorates','{"confidence":0.9,"resolvedBy":"import","refName":"log"}',6,0,NULL);
INSERT INTO edges VALUES(153,'function:f85442201d298699a22ac31b10aa022a','class:f74173ac01e5f0e1bed08f474b7ba73e','references','{"confidence":0.9,"resolvedBy":"exact-match","refName":"Square"}',20,24,NULL);
INSERT INTO edges VALUES(154,'function:f85442201d298699a22ac31b10aa022a','class:f74173ac01e5f0e1bed08f474b7ba73e','references','{"confidence":0.9,"resolvedBy":"exact-match","refName":"Square"}',20,33,NULL);
INSERT INTO edges VALUES(156,'function:f85442201d298699a22ac31b10aa022a','class:f74173ac01e5f0e1bed08f474b7ba73e','instantiates','{"confidence":0.9,"resolvedBy":"exact-match","refName":"Square"}',21,12,NULL);
INSERT INTO edges VALUES(157,'method:496dabfc265607b18b2ae8f50e960ed1','field:e828cf90fe52cfa733e36195496bb6ea','calls','{"confidence":0.9,"resolvedBy":"exact-match","refName":"items"}',11,15,NULL);
INSERT INTO edges VALUES(158,'function:d1341d521c832b672652de06edc1eedc','function:c415287c16b83edcd291de6fa946b463','calls','{"synthesizedBy":"jsx-render","via":"About"}',11,NULL,'heuristic');
INSERT INTO edges VALUES(159,'method:0b29341d41062c5ba1711832ca02283a','method:065074d25565207d1637027918aacc80','calls','{"synthesizedBy":"cpp-override","via":"area","registeredAt":"src/cpp/shape.cpp:12"}',5,NULL,'heuristic');
INSERT INTO edges VALUES(160,'method:2f7e6964ed3a09c4c976eed4d53b3d96','method:0d10434ed95791a0ba1d9aa92771e570','calls','{"synthesizedBy":"interface-impl","via":"Amount","registeredAt":"src/csharp/Ledger.cs:12"}',5,NULL,'heuristic');
INSERT INTO edges VALUES(161,'method:0d10434ed95791a0ba1d9aa92771e570','method:49dcd1784a89f7c20feac7f59524718a','calls','{"synthesizedBy":"interface-impl","via":"Amount","registeredAt":"src/csharp/Ledger.cs:19"}',12,NULL,'heuristic');
INSERT INTO edges VALUES(162,'method:47506583e4f29dc8b70490b4b8622ff7','method:7db6566d130023056ed61e675e44743c','calls','{"synthesizedBy":"interface-impl","via":"Label","registeredAt":"src/csharp/Ledger.cs:20"}',13,NULL,'heuristic');
INSERT INTO edges VALUES(163,'method:8bc91d238fa70bd653aa0315414fcbfe','method:c529163609a31e0a9ea62be99d921af8','calls','{"synthesizedBy":"interface-impl","via":"sound","registeredAt":"src/java/cov/Dog.java:6"}',4,NULL,'heuristic');
INSERT INTO edges VALUES(164,'method:71540100da33120ac5619b792a756442','method:c529163609a31e0a9ea62be99d921af8','calls','{"synthesizedBy":"interface-impl","via":"sound","registeredAt":"src/java/cov/Dog.java:6"}',4,NULL,'heuristic');
INSERT INTO edges VALUES(165,'method:691c2a4590189fc8bcb5b655238b7e96','method:440227f1abac63deb1c24eaaf0d55cce','calls','{"synthesizedBy":"interface-impl","via":"shift","registeredAt":"src/kotlin/Zoo.kt:12"}',8,NULL,'heuristic');
INSERT INTO edges VALUES(166,'method:25e017c365b01ebf4862d1a827a54251','method:d7a05ad04e58dc2672f4eac0fbe9ed40','calls','{"synthesizedBy":"interface-impl","via":"feed","registeredAt":"src/kotlin/Zoo.kt:13"}',4,NULL,'heuristic');
INSERT INTO edges VALUES(167,'method:f7f328a1f5208bb49949f6250cfeab41','method:90e2d82a912af8896f5ee0a6a6f1a4d5','calls','{"synthesizedBy":"interface-impl","via":"label","registeredAt":"src/scala/Shop.scala:12"}',8,NULL,'heuristic');
INSERT INTO edges VALUES(168,'method:574ec8ad016447ebecbfd30ebc261362','method:cf1a518b70d9ec242b95531ff6269a69','calls','{"synthesizedBy":"interface-impl","via":"price","registeredAt":"src/scala/Shop.scala:13"}',4,NULL,'heuristic');
INSERT INTO edges VALUES(169,'method:b1f68b015dc4b0a2280e76311b850b5a','method:c45312e63d3e84d2c63e5fa116077088','calls','{"synthesizedBy":"interface-impl","via":"water","registeredAt":"src/swift/Garden.swift:10"}',6,NULL,'heuristic');
INSERT INTO edges VALUES(170,'method:5d0943d27416c726fa67260a6090df20','method:18c44c380fe5bc0b19196abcf34e4130','calls','{"synthesizedBy":"interface-impl","via":"area","registeredAt":"src/ts/shapes.ts:9"}',5,NULL,'heuristic');
INSERT INTO edges VALUES(171,'method:8e03dc9b0aa566fad75eacd2a1aa88d3','method:1bf30bc0739c597f5c76278ff83be991','calls','{"synthesizedBy":"interface-impl","via":"describe","registeredAt":"src/ts/shapes.ts:12"}',6,NULL,'heuristic');
INSERT INTO edges VALUES(172,'method:a6d6036576e54ae5f666c046d7f47015','method:17d817ce728de8e0f867c7adac9f2b2e','calls','{"synthesizedBy":"interface-impl","via":"name","registeredAt":"src/ts/shapes.ts:15"}',2,NULL,'heuristic');
INSERT INTO edges VALUES(173,'function:a6a1f926e933634f309625d1748f96c5','route:src/ts/nav.tsx:14:/about','navigates','{"synthesizedBy":"react-router-link","href":"/about","navMethod":"link","registeredAt":"src/ts/nav.tsx:8"}',8,NULL,'heuristic');
INSERT INTO unresolved_refs VALUES(4,'function:645161dc82eba1a211da31f011791d88','Circle::Circle/0','calls',16,4,NULL,'src/cpp/shape.cpp','cpp','failed','Circle');
INSERT INTO unresolved_refs VALUES(15,'method:c529163609a31e0a9ea62be99d921af8','Override','decorates',6,2,NULL,'src/java/cov/Dog.java','java','failed','Override');
INSERT INTO unresolved_refs VALUES(19,'file:src/js/main.js','require','calls',1,18,NULL,'src/js/main.js','javascript','failed','require');
INSERT INTO unresolved_refs VALUES(23,'function:abca4f679782194f5335e769ff3b22de','xs.reduce','calls',2,9,NULL,'src/js/sum.js','javascript','failed','reduce');
INSERT INTO unresolved_refs VALUES(46,'file:src/ts/nav.tsx','react-router-dom','imports',1,0,NULL,'src/ts/nav.tsx','tsx','failed','react-router-dom');
INSERT INTO unresolved_refs VALUES(47,'file:src/ts/nav.tsx','Link','imports',1,9,NULL,'src/ts/nav.tsx','tsx','failed','Link');
INSERT INTO unresolved_refs VALUES(48,'file:src/ts/nav.tsx','Route','imports',1,15,NULL,'src/ts/nav.tsx','tsx','failed','Route');
INSERT INTO unresolved_refs VALUES(49,'file:src/ts/nav.tsx','Routes','imports',1,22,NULL,'src/ts/nav.tsx','tsx','failed','Routes');
INSERT INTO unresolved_refs VALUES(55,'file:src/ts/shapes.ts','./theme.css','imports',2,0,NULL,'src/ts/shapes.ts','typescript','failed','theme');
