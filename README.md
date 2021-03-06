## dovi_tool [![Tests](https://github.com/quietvoid/dovi_tool/workflows/Tests/badge.svg)](https://github.com/quietvoid/dovi_tool/actions?query=workflow%3ATests) [![Artifacts](https://github.com/quietvoid/dovi_tool/workflows/Artifacts/badge.svg)](https://github.com/quietvoid/dovi_tool/actions?query=workflow%3AArtifacts)
### Options
* `-m`, `--mode` Sets the mode for RPU processing.
  * Default (no mode) - Copies the RPU untouched.
  * `0` - Parses the RPU, rewrites it untouched.
  * `1` - Converts the RPU to be MEL compatible.
  * `2` - Converts the RPU to be profile 8.1 compatible.

* `-c`, `--crop` Set active area offsets to 0 (meaning no letterbox bars)

### Commands

#### convert
Converts RPU within a single layer HEVC file.  
The enhancement layer can be discarded using `--discard`

* Convert to 8.1 and discard EL: `dovi_tool -m 2 convert --discard file.hevc`
#### demux
Rust port of yusesope's python tool. Credits goes to them.  
Demuxes single track dual layer Dolby Vision into Base layer and Enhancement layer files.

* `dovi_tool demux file.hevc`
* `ffmpeg -i input.mkv -c:v copy -vbsf hevc_mp4toannexb -f hevc - | dovi_tool demux -`
* Convert RPU to 8.1: `dovi_tool -m 2 demux file.hevc`

#### extract-rpu
Extracts Dolby Vision RPU from an single track dual layer encoded file.
Supports profiles 5, 7, and 8.  
Input can be piped.

* `dovi_tool extract-rpu video.hevc`
* FEL to MEL example: `dovi_tool -m 1 extract-rpu video.hevc`

#### inject-rpu
Interleaves RPU NAL units between slices in an encoded HEVC file.

* `dovi_tool inject-rpu -i video.hevc --rpu-in RPU.bin`

#### editor
Edits a RPU according to a JSON config.  
See examples in `assets` folder.

* `dovi_tool editor -i RPU.bin -j assets/editor_examples/mode.json --rpu-out RPU_mode2.bin`

#### info
Prints the parsed RPU data for a specific frame.

* `dovi_tool info -i RPU.bin -f 0`  

&nbsp;

Build artifacts can be found in the Github Actions.  
More features may or may not be added in the future.
