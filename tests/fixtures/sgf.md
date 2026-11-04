# Go records

The first game’s final main-line position is shown by default:

```sgf
(;FF[4]GM[1]SZ[9]GN[Corner study]
AB[cc][dc]AW[cd]
;B[dd];W[ed]
(;B[ec]TR[ec]LB[dd:A])
(;B[ce]))
```

## Earlier position

```sgf move=1
(;FF[4]GM[1]SZ[9]AB[cc][dc]AW[cd];B[dd];W[ed];B[ec])
```

## Setup and annotations

> ```sgf move=0
> (;FF[4]GM[1]SZ[9]AB[cc][dd]AW[cd][dc]TR[cc]SQ[dc]LB[dd:A]AR[aa:cc])
> ```

## Collections

```sgf game=2
(;FF[4]GM[1]SZ[9];B[cc])
(;FF[4]GM[1]SZ[9];B[dd];W[ff])
```

Invalid records and out-of-range selectors retain source:

```sgf move=99
(;FF[4]GM[1]SZ[9];B[cc])
```

Use `--show-sgf-source` for selectable SGF beneath boards. Non-graphics terminals
and text exports retain the original record instead of a board image.
