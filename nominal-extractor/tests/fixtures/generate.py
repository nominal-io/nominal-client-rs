import json,tempfile
from pathlib import Path
from nominal.experimental.extractor._context import ManifestExtractorContext
from nominal import ts
with tempfile.TemporaryDirectory() as d:
 p=Path(d); c=ManifestExtractorContext(output_dir=p,_env={},_input_dir=p)
 for name in ['data.csv','data.avro.gz','log.jsonl','cam.mp4']:(p/name).write_text('metadata-only fixture')
 c.add_tabular(p/'data.csv',tag_columns={'vehicle':'veh_id'},channel_prefix='a/',timestamp_column='ts',timestamp_type=ts.Relative('milliseconds',start=-1))
 c.add_tabular(p/'data.csv',channel_prefix='b/')
 c.add_avro_stream(p/'data.avro.gz',timestamp_type=ts.Epoch('nanoseconds'))
 c.add_journal_json(p/'log.jsonl',timestamp_column='t',timestamp_type=ts.Epoch('seconds'))
 c.add_video(p/'cam.mp4',channel='start',start=-1)
 c.add_video(p/'cam.mp4',channel='end',start=0,ending_timestamp=1234567891)
 c.add_video(p/'cam.mp4',channel='rate',start=0,true_frame_rate=59.94)
 c.add_video(p/'cam.mp4',channel='factor',start=0,scale_factor=-2.0)
 c.add_video(p/'cam.mp4',channel='frames',frame_timestamps=[-1,1700000000123456789])
 print(json.dumps(c.build_manifest(),indent=2))
