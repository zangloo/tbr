# tbr(terminal ebook reader)

## About

`tbr` is a simple e-book reader in terminal.By now, support txt, html and epub.

## Build

    cargo build --release

## Configuration

Config files store in .config/ter/ter.toml. Themes configuration files store in ~/.config/ter/themes/. Files will auto
generated if not exists.

## Key binding

| Function                  | Key mappings                  |
|---------------------------|-------------------------------|
| Next page                 | space,page down               |
| Previous page             | page up                       |
| Search                    | '/'                           |
| Search Next               | 'n'                           |
| Search backward           | 'N'                           |
| Inner book dialog         | 'b'                           |
| History dialog            | 'h'                           |
| Select chapter            | 'c'                           |
| Select theme              | 't'                           |
| Show version              | 'v'                           |
| Next line                 | down                          |
| Previous line             | up                            |
| Back prev position        | left                          |
| Forward to next position  | right                         |
| Goto start of chapter     | home                          |
| Goto end of chapter       | end                           |
| Goto line                 | 'g'                           |
| Navigate to next link     | tab                           |
| Navigate to prev link     | shift + tab                   |
| Open link                 | left click/enter on highlight |
| Next chapter              | ^D                            |
| Previous chapter          | ^B                            |
| Switch view mode han<=>xi | ^X                            |
| Quit                      | 'q'                           |

## License

GPLv2
