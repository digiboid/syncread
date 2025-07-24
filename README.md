# Syncread

Assistant for synchronized manga reading using mpv. Displays your current position relative to other users.

## Prerequisites
- mpv

## Installation
Download the latest binary from [Releases](https://github.com/digiboid/syncread/releases) or build from source.

## Usage
### Server
```bash
syncread server --bind 0.0.0.0:8080
```

### Client  
```bash
syncread client --server ip:8080 --minimal --user-id username path/to/folder
```

### Custom MPV Path
If MPV is not in your PATH, you can specify the binary location:
```bash
# Windows example
syncread client --server ip:8080 --mpv-path "C:\Program Files\MPV\mpv.exe" --user-id username path/to/folder

# Linux example  
syncread client --server ip:8080 --mpv-path /opt/mpv/bin/mpv --user-id username path/to/folder
```

# Features

## Known Issues
- Flickering on Windows terminal


## Building from Source
```bash
git clone https://github.com/digiboid/syncread.git
cd syncread
cargo build --release
```
