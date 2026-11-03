# Drag & Drop Debugging Guide

## What Was Fixed

### 1. **Instant Visual Feedback** ✅
- **Problem**: Drag handlers only on drop zone element
- **Solution**: Attached to entire `document` for instant response
- **Uses drag counter** to track nested enter/leave events

### 2. **Multiple Event Listeners** ✅
- Listening to 3 different event structures:
  1. `tauri://file-drop` with `string[]` payload
  2. `tauri://file-drop` with `{ paths: string[] }` payload
  3. `tauri://drag-drop` (fallback for older versions)

### 3. **Console Logging** ✅
- All events now log to browser console
- Easy to see what's firing and what's not

## How to Test

### Step 1: Open Dev Tools
```bash
cd apps/dctx-desktop
bun run tauri:dev
```

**In the app window:**
- Right-click → Inspect Element
- OR Press: `Cmd+Option+I` (macOS) / `Ctrl+Shift+I` (Windows/Linux)
- Go to **Console** tab

### Step 2: Watch Console While Testing

You should see these logs on startup:
```
Setting up Tauri file drop listeners...
✅ All Tauri event listeners registered
Loaded desktop config:
  Config dir: ...
  Cache dir: ...
  File limit: 10000
  Core config inherited: text_output=true
```

### Step 3: Test Drag Feedback (Should Be INSTANT)

**Drag a folder from Finder over the window:**

**Expected Console Output:**
```
Drag enter - counter: 1
```

**Expected Visual:**
- Drop zone border turns blue immediately
- Border thickens
- Background becomes light blue

**When you drag out:**
```
Drag leave - counter: 0
```

### Step 4: Test Drop

**Drop the folder:**

**Expected Console Output:**
```
Drop event fired on DOM
✅ File drop event (tauri://file-drop): [Event object]
  Payload: ["/path/to/folder"]
  Setting path: /path/to/folder
```

**Expected Visual:**
- Path appears in input field
- Status bar updates: "Dropped: /path/to/folder"

## Common Issues & Solutions

### Issue 1: Visual Feedback Slow
**Symptom**: Border doesn't change immediately when dragging

**Check:**
- Console shows "Drag enter" log?
- If YES: CSS issue (check `.drag-over` class)
- If NO: Event handler not attached

**Fix:**
```bash
# Hard refresh
Cmd+Shift+R (macOS)
Ctrl+Shift+R (Windows/Linux)
```

### Issue 2: Drop Does Nothing
**Symptom**: Visual feedback works, but drop doesn't set path

**Check Console:**

**If you see:** "Drop event fired on DOM" but NO Tauri event
- Tauri event not firing
- Check permissions (see below)

**If NO console logs at all:**
- Dev tools not open
- Console cleared
- JavaScript error (check for red errors)

### Issue 3: Tauri Event Not Firing

**Possible causes:**
1. **dragDropEnabled not set** (should be in tauri.conf.json line 49)
2. **macOS permissions** (first run might need authorization)
3. **Event name changed** (Tauri version mismatch)

**Check tauri.conf.json:**
```json
{
  "app": {
    "windows": [{
      "dragDropEnabled": true  // ← Must be true
    }]
  }
}
```

**Check macOS Security:**
1. System Settings → Privacy & Security
2. Files and Folders
3. Look for "dctx Desktop" or "dctx-desktop"
4. Ensure access is granted

### Issue 4: Wrong Path Type Dropped

**Symptom**: Console shows event but path is empty/wrong

**Check payload structure in console:**
```javascript
// Look for:
Payload: ["/path/to/folder"]           // ✅ Good
Payload: { paths: ["/path/to/folder"] } // ✅ Also good
Payload: undefined                     // ❌ Bad - wrong event
Payload: ["/path/to/file.txt"]         // ❌ Bad - dropped file not folder
```

## Manual Test Commands

### Test 1: Verify Config
```typescript
// In browser console:
document.querySelector('#status').textContent
// Should show: "Ready - Drag & drop a folder to start"
```

### Test 2: Test setPath Function
```typescript
// In browser console:
document.querySelector('#path-input').value = "/tmp"
// Should immediately update input field
```

### Test 3: Trigger Drag Feedback Manually
```typescript
// In browser console:
document.querySelector('#drop-zone').classList.add('drag-over')
// Should see blue border immediately
```

## Expected Console Output (Full Flow)

```
[Startup]
Setting up Tauri file drop listeners...
✅ All Tauri event listeners registered
Loaded desktop config: ...
System theme: dark mode
Ready - Drag & drop a folder to start

[Drag Enter]
Drag enter - counter: 1

[Drag Leave]
Drag leave - counter: 0

[Drop]
Drop event fired on DOM
✅ File drop event (tauri://file-drop): {...}
  Payload: ["/path/to/Desktop"]
  Setting path: /path/to/Desktop
Dropped: /path/to/Desktop
```

## Still Not Working?

### Collect Debug Info:

1. **Take screenshot of console** (with error messages if any)
2. **Note the behavior:**
   - Does visual feedback work?
   - Does console show drag events?
   - Does console show Tauri events?
   - What happens when you drop?

3. **Try the browse button:**
   - Click "📂 Browse Files"
   - Does native picker open?
   - Does selecting a folder work?
   - This tests if the file picker plugin works

4. **Check Tauri version:**
```bash
cd src-tauri
cargo tree | grep tauri-runtime
```

Should be:
```
tauri v2.x.x
tauri-runtime v2.x.x
```

## Quick Fixes to Try

### Fix 1: Full Clean Build
```bash
cd apps/dctx-desktop
rm -rf dist/ node_modules/ src-tauri/target/
bun install
bun run build
cargo build --manifest-path src-tauri/Cargo.toml
bun run tauri:dev
```

### Fix 2: Rebuild Frontend Only
```bash
bun run build
# Then restart tauri:dev
```

### Fix 3: Clear Browser Cache
In dev tools:
- Application tab
- Clear storage
- Hard refresh (Cmd+Shift+R)
