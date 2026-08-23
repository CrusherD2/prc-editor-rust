# Performance Optimizations for PRC Editor

## System-Level Optimizations

### 1. **Windows Icon Cache Optimization**
Based on [Winaero's guide](https://winaero.com/change-icon-cache-size-windows-10/), increase icon cache size:

1. Open Registry Editor (`regedit`)
2. Navigate to: `HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer`
3. Create new String Value: `MaxCachedIcons`
4. Set value to `4096` (4MB) or `8192` (8MB)
5. Restart your system

### 2. **7-max Performance Booster**
From [7-max.com](https://7-max.com/), 7-max can improve application performance by 10-20%:

1. Download 7-max 24.01 for Windows
2. Run your PRC editor through 7-max
3. **Warning**: Can be unstable for some systems - test carefully

### 3. **Memory Management**
- Close other applications when working with large files
- Ensure sufficient RAM (8GB+ recommended)
- Use SSD storage for faster file access

## Application-Level Optimizations

### 1. **Lazy Loading**
The application now implements lazy loading:
- Tree nodes are only built when expanded
- Children are cleared after processing to free memory
- Virtual scrolling limits visible items

### 2. **Caching System**
- Node cache reduces repeated lookups
- Tree rebuild cooldown prevents excessive updates
- Frame-based throttling for expensive operations

### 3. **Batch Processing**
- Large operations are processed in batches
- Progress indicators show operation status
- Memory is freed between batches

## Code Optimizations Implemented

### 1. **Tree Rebuild Optimization**
```rust
// Only rebuild tree when necessary
if self.should_rebuild_tree() {
    self.build_tree_items();
    self.last_tree_rebuild_frame = self.frame_count;
    self.tree_items_dirty = false;
}
```

### 2. **Virtual Scrolling**
```rust
// Limit visible items for large lists
let visible_range = self.calculate_visible_range(ui, &node.children);
for (i, child) in node.children.iter().enumerate().skip(visible_range.0).take(visible_range.1) {
    // Render only visible items
}
```

### 3. **Memory Management**
```rust
// Clear children after processing to free memory
child.children.clear();
child.children_built = false;
```

## Performance Settings

The application includes configurable performance settings:

- `enable_virtual_scrolling`: Enable virtual scrolling for large trees
- `enable_node_caching`: Enable node caching
- `max_tree_depth`: Maximum tree depth to render (default: 8)
- `max_visible_items`: Maximum visible items in lists (default: 100)

## Tips for Large Files

1. **Start with Root Node**: Always begin by selecting the root node to see the overall structure
2. **Expand Selectively**: Only expand nodes you need to work with
3. **Use Search**: Use keyboard navigation (arrow keys) to quickly navigate large trees
4. **Batch Operations**: Use bulk enable/disable operations instead of individual changes
5. **Close Unused Panels**: Close the label editor when not needed

## Troubleshooting Slow Performance

1. **Check Memory Usage**: Monitor Task Manager for high memory usage
2. **Reduce Tree Depth**: Lower `max_tree_depth` setting if needed
3. **Clear Cache**: The application automatically clears cache when needed
4. **Restart Application**: If performance degrades, restart the application
5. **Use Smaller Files**: For very large files, consider splitting them

## Expected Performance Improvements

With these optimizations, you should see:
- **10-20% faster loading** with 7-max
- **Reduced memory usage** with lazy loading
- **Smoother UI** with virtual scrolling
- **Faster navigation** with caching
- **Better responsiveness** with throttled operations

## System Requirements

For optimal performance with large files:
- **RAM**: 8GB+ recommended
- **Storage**: SSD preferred
- **CPU**: Multi-core processor
- **OS**: Windows 10/11 with latest updates 