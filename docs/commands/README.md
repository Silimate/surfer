# Commands

There are two ways to invoke commands in Surfer.

* Load a file one start-up using the ``--command-file`` argument.
* Press space and type the command (with fuzzy matching)

Not all commands are available unless a file is loaded. Also, some commands are not available in the WASM-build (browser/VS Code extension).

## Waveform/transaction loading and reloading

* ``load_file <FILE_NAME>``

    Load a file.
* ``switch_file <FILE_NAME>``

    Load file, but keep waveform view.
* ``load_url <FILE_NAME>``

    Loads an URL.
* ``reload`` (not on WASM)
* ``config_reload``
* ``remove_unavailable``

## State files

* ``load_state``
* ``save_state``
* ``save_state_as``

## Add variable/transaction items

* ``scope_select``
* ``stream_select``
* ``variable_add <VARIABLE_NAME>``, ``generator_add  <GENERATOR_NAME>``

## Add other items

* ``divider_add <NAME>``
* ``timeline_add``

## Controlling item appearance

* ``item_focus``
* ``item_set_color <COLOR_NAME>``
* ``item_set_background_color <COLOR_NAME>``
* ``item_set_format <FORMAT_NAME>``
* ``item_unset_color``

  Reset to default color.

* ``item_unset_background_color``

  Reset to default background color.

* ``item_unfocus``

  Remove focus from currently focused item.

* ``item_rename``
* ``scope_add``, ``stream_add``
* ``scope_add_recursive``
* ``theme_select <THEME_NAME>``
* ``variable_add_from_scope``
* ``generator_add_from_stream``

## Navigation

* ``zoom_fit``

  Zoom to display the full simulation.

* ``zoom_in``
* ``zoom_out``
* ``scroll_to_start``,  ``goto_start``
* ``scroll_to_end``, ``goto_end``
* ``transition_next``

  Move cursor to next transition of focused item. Scroll if not visible.

* ``transition_previous``

  Move cursor to previous transition of focused item. Scroll if not visible.

* ``transaction_next``
* ``transaction_prev``

## UI control

* ``show_controls``
* ``show_mouse_gestures``

  Show mouse gesture help window.

* ``show_quick_start``
* ``show_logs``

  Show log window.

* ``toggle_menu``

  Toggle visibility of menu. If not visible, there will be a burger menu in the toolbar.

* ``toggle_side_panel``

* ``toggle_fullscreen``

  Toggle fullscreen view.

* ``toggle_tick_lines``
* ``variable_set_name_type``
* ``variable_force_name_type``
* ``preference_set_clock_highlight``
* ``preference_set_hierarchy_style``
* ``preference_set_arrow_key_bindings``

## Cursor and markers

* ``goto_cursor``

  Go to the location of the main cursor. If off screen, scroll to it.

* ``goto_marker <MARKER_NAME> | #<MARKER_NUMBER>``

  Go to the location of the main cursor. If off screen, scroll to it.

* ``cursor_set <TIME>``

  Move cursor to given time.

* ``marker_set  <MARKER_NAME> | #<MARKER_NUMBER>``

  Add/set marker to location of cursor.

* ``marker_remove <MARKER_NAME> | #<MARKER_NUMBER>``

  Remove marker.

* ``show_marker_window``

  Display window with markers and differences between markers

## Interactive simulation

* ``pause_simulation``
* ``unpause_simulation``

## Viewports

* ``viewport_add``
* ``viewport_remove``

## Waveform control protocol (WCP)

* ``wcp_server_stop`` (not WASM)
* ``wcp_server_start`` (not WASM)

## Other

* ``copy_value``
* ``undo``
* ``redo``
* ``exit`` (not WASM)
