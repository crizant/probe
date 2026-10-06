Output demo/demo.gif

Set Shell "fish"
Set Theme "Catppuccin Macchiato"
Set FontSize 15
Set Width 1300
Set Height 650
Set TypingSpeed 30ms
Set CursorBlink false

Hide

Type "set -g fish_greeting ''"
Enter
Type "set -g fish_color_command c6a0f6"
Enter
Type "set -g fish_color_option f5a97f"
Enter
Type "set -g fish_color_param cad3f5"
Enter
Type "set -g fish_color_quote a6da95"
Enter
Type "set -g fish_color_error ed8796"
Enter
Type "set -g fish_color_valid_path 8aadf4 --underline"
Enter

Ctrl+L
Show

Sleep 750ms

Type "probe request run examples/dummyjson.yml items/7/items/1 --environment Development --var todoId=3 --expect status=200"

Sleep 400ms
Enter

Sleep 5s