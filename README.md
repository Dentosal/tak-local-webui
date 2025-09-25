# Local webui for playing against tak bots

Tak bots on [playtak.com](https://playtak.com) not available all the time, and there's a limited selection of them.
This project provides a simple webui for playing against tak bots locally.
It uses [ptn.ninja](https://ptn.ninja) for rendering the board, and supports all TEI engines.

## Usage

```bash
cargo build --release
./target/release/tak-bot-webui path/to/engine1 path/to/engine2
```

To get some engines, you can run

```
python3 build_engines.py
./target/release/tak-bot-webui -F enginelist.txt
```