# tbr(A terminal and gui ebook reader)

## About

`tbr` is a simple e-book reader in terminal and gtk4(>4.10).By now, support txt, html, haodoo and epub.
it can also render text as chinese tradition style(top to bottom and right to left).

## Build

    cargo build --release

## Build without GUI support

    cargo build --release --no-default-features

## Configuration

Config files store in .config/tbr/tbr.toml. Themes configuration files store in ~/.config/ter/themes/. Files will auto
generated if not exists.

## Key binding for terminal

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

## Screenshots

### In terminal:
![Screenshot_2022-05-25_21-05-44](https://user-images.githubusercontent.com/10681694/170268007-6959b9d4-34d8-49fa-9af4-a4e904f6223a.png)

### In Gui(Xi)
![Screenshot_2024-07-06_11-13-08](https://github.com/zangloo/tbr/assets/10681694/04727e86-f7fe-446e-b727-bdaaf789d373)

### In Gui(Han)
![Screenshot_2024-07-06_11-14-24](https://github.com/zangloo/tbr/assets/10681694/ddd26bdd-aec1-445f-b0a6-c2b2ac948604)
