# odin_adsb

## Running jet1090

(1) single client, multiple reconnects

```sh
mkfifo /tmp/adsb_fifo
nc -l -k -p 30003 < /tmp/adsb_fifo&
jet1090 -v rtlsdr:// > /tmp/adsb_fifo
```

(2) multiple clients, multiple reconnects

```sh
mkfifo /tmp/adsb_fifo
socat TCP-LISTEN:30003,reuseaddr,fork,cool-write 'PIPE:/tmp/adsb_fifo!!PIPE:/tmp/adsb_fifo'&
jet1090 -v rtlsdr:// > /tmp/adsb_fifo
```

Note this blocks jet1090 until the first connection is made. The `cool-write` option is essential to avoid
socat shutting down and causing a broken pipe when a client disconnects
