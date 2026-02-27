# Testing the subsystem on `foc-devnet`:
The system for testing relies on:
- `foc-devnet`, commit: `874af2d` (adds debug logs for curio in foc-devnet)
- `curio`, commit: `3192924a` (adds debug logs and task timing bookeeping metadata)
- `synapse-sdk`, commit: `f7e4b25` (adds tools for variable size deal making with curio nodes)

## System Setup
Run foc-devnet in a notest format:
```sh
cargo run -- start --parallel --notest
```

Install python and cqlsh:
```sh
sudo apt install -y python3.12-venv
python3 -m venv ~/.foc-devnet/venv
~/.foc-devnet/venv/bin/pip install cqlsh
```

Once the foc-devnet is up, we can check the situation with postgresql database, we expect 
`(needs_save_cache = FALSE, save_cache_task_id = NULL)` for items that do not require caching or are already cached:

## Initialization, clean slate
```sh
DEVNET_INFO=~/.foc-devnet/state/latest/devnet-info.json
RUN_ID=$(jq -r '.info.run_id' $DEVNET_INFO)
YUGABYTE_CONTAINER="foc-${RUN_ID}-yugabyte-1"
YCQL_PORT=$(docker port $YUGABYTE_CONTAINER 9042/tcp | head -1 | cut -d: -f2)
echo $YUGABYTE_CONTAINER
```

```sh
docker exec -it -e PGPASSWORD='yugabyte' $YUGABYTE_CONTAINER \
/yugabyte/bin/ysqlsh -h localhost -p 5433 -U yugabyte -d yugabyte -c \
"SELECT pr.id, pr.piece_cid, pr.needs_save_cache, pr.caching_task_started, pr.caching_task_completed FROM curio.pdp_piecerefs pr"

~/.foc-devnet/venv/bin/cqlsh localhost $YCQL_PORT -u cassandra -p cassandra \
  -e "SELECT * FROM curio.pdp_cache_layer;"
```

We see:
```sh
 id  |                            piece_cid                             | needs_save_cache |     caching_task_started      |    caching_task_complet
ed     
-----+------------------------------------------------------------------+------------------+-------------------------------+------------------------
-------
 101 | baga6ea4seaqofpvdtfe2jyeiwsngurkh4tv7jt44l32cdejcuehjnrcfgkat2di | f                | 2026-02-20 08:15:39.427793+00 | 2026-02-20 08:15:39.428
334+00
   1 | baga6ea4seaqj74lmbgttmobnynivupwkfu53crxl5sxmauivxpnw3rtpbbdoyaq | f                | 2026-02-20 08:12:22.831085+00 | 2026-02-20 08:12:22.831
791+00
 201 | baga6ea4seaqadgmozxl5hsyhl3alk2234t3uwwy7uv2qei6h6avzlbwbgqwqkoi | t                | 2026-02-20 08:16:17.069485+00 | 
(3 rows)


 piececid | layerindex | leafindex | leaf
----------+------------+-----------+------

(1 row)
```
There are three pieces:
- One that came through via pdptool testing uploads/downloads during foc-devnet setup phase.
- One that we uploaded 10MB
- One that we uploaded 120MB

### We see curio logs as well:
For 10MB piece:
```sh
2026-02-20T08:15:39.428Z        DEBUG   pdp     pdp/task_save_cache.go:69       PDPv0_SaveCache starting        {"taskID": 513, "pieceCID": "baga6ea4seaqofpvdtfe2jyeiwsngurkh4tv7jt44l32cdejcuehjnrcfgkat2di", "rawSize": 10485760}
2026-02-20T08:15:39.428Z        DEBUG   pdp     pdp/task_save_cache.go:85       PDPv0_SaveCache: piece below cache threshold, skipping layer build {"pieceCID": "baga6ea4seaqofpvdtfe2jyeiwsngurkh4tv7jt44l32cdejcuehjnrcfgkat2di", "rawSize": 10485760, "threshold": 104857600}
2026-02-20T08:15:39.428Z        DEBUG   pdp     pdp/task_save_cache.go:146      PDPv0_SaveCache: marking task complete in DB    {"taskID": 513, "pieceCID": "baga6ea4seaqofpvdtfe2jyeiwsngurkh4tv7jt44l32cdejcuehjnrcfgkat2di"}
```

But for 120MB piece:
```sh
2026-02-20T08:16:13.790Z        DEBUG   pdp     pdp/task_save_cache.go:69       PDPv0_SaveCache starting        {"taskID": 248, "pieceCID": "baga6ea4seaqadgmozxl5hsyhl3alk2234t3uwwy7uv2qei6h6avzlbwbgqwqkoi", "rawSize": 125829120}
2026-02-20T08:16:13.790Z        ERROR   harmonytask     harmonytask/task_type_handler.go:237    Do() returned error     {"type": "PDPv0_SaveCache", "id": "248", "error": "failed to check if piece has PDP layer: scanning highest layer for PDP cache layer (P:0x01559120258080c0031601998ecdd7d3cb075ec0b56b5be4f74b5b1fa5750223c7f02b9586c1342d0539) bafkzcibfqcamaaywagmy5tox2pfqoxwawvvvxzhxjnnr7jlvair4p4blswdmcnbnau4q: gocql: no hosts available in the pool", "errorVerbose": "failed to check if piece has PDP layer:\n    github.com/filecoin-project/curio/tasks/pdp.(*TaskPDPSaveCache).Do\n        /workspace/source/tasks/pdp/task_save_cache.go:90\n  - scanning highest layer for PDP cache layer (P:0x01559120258080c0031601998ecdd7d3cb075ec0b56b5be4f74b5b1fa5750223c7f02b9586c1342d0539) bafkzcibfqcamaaywagmy5tox2pfqoxwawvvvxzhxjnnr7jlvair4p4blswdmcnbnau4q:\n    github.com/filecoin-project/curio/market/indexstore.(*IndexStore).GetPDPLayerIndex\n        /workspace/source/market/indexstore/indexstore.go:753\n  - gocql: no hosts available in the pool"}
2026-02-20T08:16:16.945Z        DEBUG   pdp     pdp/task_save_cache.go:69       PDPv0_SaveCache starting        {"taskID": 248, "pieceCID": "baga6ea4seaqadgmozxl5hsyhl3alk2234t3uwwy7uv2qei6h6avzlbwbgqwqkoi", "rawSize": 125829120}
2026-02-20T08:16:16.945Z        ERROR   harmonytask     harmonytask/task_type_handler.go:237    Do() returned error     {"type": "PDPv0_SaveCache", "id": "248", "error": "failed to check if piece has PDP layer: scanning highest layer for PDP cache layer (P:0x01559120258080c0031601998ecdd7d3cb075ec0b56b5be4f74b5b1fa5750223c7f02b9586c1342d0539) bafkzcibfqcamaaywagmy5tox2pfqoxwawvvvxzhxjnnr7jlvair4p4blswdmcnbnau4q: gocql: no hosts available in the pool", "errorVerbose": "failed to check if piece has PDP layer:\n    github.com/filecoin-project/curio/tasks/pdp.(*TaskPDPSaveCache).Do\n        /workspace/source/tasks/pdp/task_save_cache.go:90\n  - scanning highest layer for PDP cache layer (P:0x01559120258080c0031601998ecdd7d3cb075ec0b56b5be4f74b5b1fa5750223c7f02b9586c1342d0539) bafkzcibfqcamaaywagmy5tox2pfqoxwawvvvxzhxjnnr7jlvair4p4blswdmcnbnau4q:\n    github.com/filecoin-project/curio/market/indexstore.(*IndexStore).GetPDPLayerIndex\n        /workspace/source/market/indexstore/indexstore.go:753\n  - gocql: no hosts available in the pool"}
...
```

This is very weird since the same gocql was able to get the sessions made properly and do the creation of `curio.pdp_cache_layer`. However, the same `session` object says "no host available in the pool" when queried.

