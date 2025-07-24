# Syncread

Assistant for synchronized manga reading using mpv. Displays your current position relative to other users.

## Prerequisites
- **MPV** must be installed and available in PATH

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

# Features

## Known Issues
- When switching pages quickly, syncread may temporarily report being on the first page
- Flickering on windows terminal

## Building from Source
```bash
git clone https://github.com/digiboid/syncread.git
cd syncread
cargo build --release
```
