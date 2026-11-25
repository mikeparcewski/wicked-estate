-- NOT codegraph output. codegraph 1.6.4 emits no `overrides`, `returns`, `type_of` or `exports`
-- edges for the languages estate hands over (see codegraph::coverage_tests::NOT_EMITTED), yet
-- estate maps them. These rows, in the recording's own row shape, are applied after
-- codegraph-1.6.4.sql for the gate store only, so the import writes each of those kinds through
-- its real route and the consumers decode them (estate#595). `resolvedBy` says `synthetic`, so
-- the store shows `codegraph:synthetic` on each one. Ids and lines are from the recording and
-- repo/src/ts/shapes.ts.
INSERT INTO edges VALUES(9001,'method:1bf30bc0739c597f5c76278ff83be991','method:8e03dc9b0aa566fad75eacd2a1aa88d3','overrides','{"confidence":0.9,"resolvedBy":"synthetic","refName":"describe"}',12,11,NULL);
INSERT INTO edges VALUES(9002,'function:f85442201d298699a22ac31b10aa022a','class:f74173ac01e5f0e1bed08f474b7ba73e','returns','{"confidence":0.9,"resolvedBy":"synthetic","refName":"Square"}',20,32,NULL);
INSERT INTO edges VALUES(9003,'property:30940e3a62462c0c0fa824c3ff2c80a1','class:f74173ac01e5f0e1bed08f474b7ba73e','type_of','{"confidence":0.9,"resolvedBy":"synthetic","refName":"Square"}',8,2,NULL);
INSERT INTO edges VALUES(9004,'file:src/ts/shapes.ts','function:18435f682dce2df791235756b0e21179','exports','{"confidence":0.9,"resolvedBy":"synthetic","refName":"log"}',4,9,NULL);
