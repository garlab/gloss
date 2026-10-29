# Syntax highlighting

Fence languages select colors; indented, unknown, and oversized blocks remain
readable code. Compare with `--no-highlight`, `--light`, and `NO_COLOR=1`.

```rust
/* Multiline comments
   keep their context. */
fn greet(name: &str) -> String {
    format!("Hello, {name}!\n")
}
```

```python
def greet(name: str) -> str:
    """A multiline
    description."""
    return f"Hello, {name}!"
```

```typescript
interface Item { title: string; done: boolean }
const item: Item = { title: "Read the guide", done: false };
```

```json
{"count": 42, "enabled": true, "labels": ["café", "中文", "👩‍💻"]}
```

```sh
# No commands are executed.
for file in *.md; do
    printf '%s\n' "$file"
done
```

> ```go
> func add(a, b int) int {
>     return a + b
> }
> ```

- ```yaml
  viewer:
    theme: dark
    width: 80
  ```

```toml
[viewer]
width = 80
enabled = true
```

```unknown
This block stays complete without syntax colors.
```
