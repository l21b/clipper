<script lang="ts">
    import { invoke } from '@tauri-apps/api/core';
    import { listen, type UnlistenFn } from '@tauri-apps/api/event';
    import { getVersion } from '@tauri-apps/api/app';
    import { open as openDialog } from '@tauri-apps/plugin-dialog';
    import { onMount } from 'svelte';
    import type { ClipboardRecord, Settings } from '$lib/types';
    import SearchBar from '$lib/components/SearchBar.svelte';
    import ClipboardList from '$lib/components/ClipboardList.svelte';
    import SettingsModal from '$lib/components/SettingsModal.svelte';

    type ExportFavoritesResult = {
        count: number;
        path: string;
    };

    let records = $state<ClipboardRecord[]>([]);
    let loading = $state(false);
    let searchKeyword = $state('');
    let favoritesOnly = $state(false);
    let searchTimeout: ReturnType<typeof setTimeout>;
    let refreshInterval: ReturnType<typeof setInterval>;
    let unlistenOpenSettings: UnlistenFn | null = null;
    let unlistenOpenAbout: UnlistenFn | null = null;
    let unlistenMainWindowOpened: UnlistenFn | null = null;
    let settingsOpen = $state(false);
    let aboutOpen = $state(false);
    let appVersion = $state('0.1.0');
    let clearConfirmOpen = $state(false);
    let addFavoriteOpen = $state(false);
    let favoriteInput = $state('');
    let addFavoriteSaving = $state(false);
    let searchBarRef: { focusInput: () => void } | null = null;
    let settings = $state<Settings>({
        hotkey_modifiers: 0,
        hotkey_key: 0,
        hotkey: 'Ctrl+Shift+V',
        theme: 'system',
        keep_days: 30,
        max_records: 500,
        menu_width: 400,
        menu_height: 500,
        auto_start: false
    });

    const DEFAULT_LIMIT = 50;

    function preApplyCachedTheme() {
        if (typeof window === 'undefined') return;
        const cached = window.localStorage.getItem('clipper-theme');
        if (cached === 'light' || cached === 'dark') {
            document.documentElement.setAttribute('data-theme', cached);
        } else if (cached === 'system') {
            document.documentElement.removeAttribute('data-theme');
        }
    }

    preApplyCachedTheme();

    function listCommand(): 'get_history_records' | 'get_favorite_records' {
        return favoritesOnly ? 'get_favorite_records' : 'get_history_records';
    }

    function searchCommand(): 'search_records' | 'search_favorite_records' {
        return favoritesOnly ? 'search_favorite_records' : 'search_records';
    }

    function pageTitle(): string {
        return favoritesOnly ? '收藏' : '历史记录';
    }

    function emptyTitle(): string {
        return favoritesOnly ? '暂无收藏' : '暂无记录';
    }

    function emptyHint(): string {
        return favoritesOnly ? '点击+来添加' : '复制内容以记录';
    }

    function sortRecordsByPinnedAndTime(items: ClipboardRecord[]): ClipboardRecord[] {
        return [...items].sort((a, b) => {
            const pinDiff = Number(b.is_pinned) - Number(a.is_pinned);
            if (pinDiff !== 0) return pinDiff;
            return new Date(b.created_at).getTime() - new Date(a.created_at).getTime();
        });
    }

    function applyTheme(theme: Settings['theme']) {
        const root = document.documentElement;
        if (typeof window !== 'undefined') {
            window.localStorage.setItem('clipper-theme', theme);
        }
        if (theme === 'system') {
            // system 主题：移除 data-theme 属性，使用 CSS @media (prefers-color-scheme) 自动检测
            root.removeAttribute('data-theme');
            root.setAttribute('data-theme', 'auto');
            return;
        }
        root.setAttribute('data-theme', theme);
    }

    async function loadHistory(showLoading: boolean = true) {
        try {
            if (showLoading) {
                loading = true;
            }
            const keyword = searchKeyword.trim();
            if (keyword) {
                records = await invoke<ClipboardRecord[]>(searchCommand(), {
                    keyword,
                    limit: DEFAULT_LIMIT
                });
            } else {
                records = await invoke<ClipboardRecord[]>(listCommand(), {
                    limit: DEFAULT_LIMIT,
                    offset: 0
                });
            }
        } catch (error) {
            console.error('Failed to load history:', error);
        } finally {
            if (showLoading) {
                loading = false;
            }
        }
    }

    // 静默刷新，不显示 loading
    async function refreshHistory() {
        try {
            if (settingsOpen || clearConfirmOpen || addFavoriteOpen) {
                return;
            }
            // 搜索模式下不覆盖搜索结果
            if (searchKeyword.trim()) {
                return;
            }
            const newRecords = await invoke<ClipboardRecord[]>(listCommand(), {
                limit: DEFAULT_LIMIT,
                offset: 0
            });
            // 静默更新，不触发 loading
            records = newRecords;
        } catch (error) {
            console.error('Failed to refresh:', error);
        }
    }

    async function searchHistory(keyword: string) {
        clearTimeout(searchTimeout);
        searchTimeout = setTimeout(async () => {
            try {
                loading = true;
                if (keyword.trim()) {
                    records = await invoke<ClipboardRecord[]>(searchCommand(), {
                        keyword,
                        limit: DEFAULT_LIMIT
                    });
                } else {
                    records = await invoke<ClipboardRecord[]>(listCommand(), {
                        limit: DEFAULT_LIMIT,
                        offset: 0
                    });
                }
            } catch (error) {
                console.error('Failed to search:', error);
            } finally {
                loading = false;
            }
        }, 300);
    }

    async function handleSearch(value: string) {
        searchKeyword = value;
        await searchHistory(value);
    }

    async function resetSearchStateOnShow() {
        if (!searchKeyword.trim()) return;
        clearTimeout(searchTimeout);
        searchKeyword = '';
        await loadHistory(false);
    }

    function focusSearchInput(delayMs: number = 0) {
        setTimeout(() => {
            if (settingsOpen || clearConfirmOpen || addFavoriteOpen || aboutOpen) return;
            searchBarRef?.focusInput?.();
        }, delayMs);
    }

    async function loadSettings() {
        try {
            settings = await invoke<Settings>('get_app_settings');
            applyTheme(settings.theme);
        } catch (error) {
            console.error('Failed to load settings:', error);
        }
    }

    function saveSettings(nextSettings: Settings) {
        // 先应用主题设置（不管后端保存是否成功）
        settings = { ...nextSettings };
        applyTheme(settings.theme);
        settingsOpen = false;
        focusSearchInput(0);

        // 保存到后端（不重新加载设置，因为我们已经有了最新值）
        void invoke('save_app_settings', { settings: nextSettings })
            .then(async () => {
            await loadHistory();
            })
            .catch((error) => {
                console.error('Failed to save settings:', error);
            });
    }

    async function handleCopy(id: number) {
        const record = records.find(r => r.id === id);
        if (record) {
            try {
                await invoke('paste_record_content', { id: record.id });
            } catch (error) {
                console.error('Failed to paste record content:', error);
            }
        }
    }

    async function handleDelete(id: number) {
        try {
            await invoke('delete_clipboard_record', { id });
            records = records.filter(r => r.id !== id);
        } catch (error) {
            console.error('Failed to delete:', error);
        }
    }

    async function handleFavorite(id: number, favorite: boolean) {
        const previous = records;
        if (favoritesOnly && !favorite) {
            records = records.filter((r) => r.id !== id);
        } else {
            records = records.map((r) =>
                r.id === id ? { ...r, is_favorite: favorite } : r
            );
        }

        try {
            await invoke('set_record_favorite_state', { id, favorite });
        } catch (error) {
            records = previous;
            console.error('Failed to update favorite state:', error);
        }
    }

    async function handlePinned(id: number, pinned: boolean) {
        const previous = records;
        records = records.map((r) =>
            r.id === id ? { ...r, is_pinned: pinned } : r
        );
        records = sortRecordsByPinnedAndTime(records);

        try {
            await invoke('set_record_pinned_state', { id, pinned });
        } catch (error) {
            records = previous;
            console.error('Failed to update pinned state:', error);
        }
    }

    function openAddFavoriteDialog() {
        addFavoriteOpen = true;
        favoriteInput = '';
    }

    function closeAddFavoriteDialog() {
        if (addFavoriteSaving) return;
        addFavoriteOpen = false;
        focusSearchInput(0);
    }

    async function submitAddFavorite() {
        const text = favoriteInput.trim();
        if (!text || addFavoriteSaving) return;

        addFavoriteSaving = true;
        try {
            await invoke('add_custom_favorite_record', { content: text });
            addFavoriteOpen = false;
            favoriteInput = '';
            await loadHistory();
            focusSearchInput(0);
        } catch (error) {
            console.error('Failed to add custom favorite record:', error);
        } finally {
            addFavoriteSaving = false;
        }
    }

    async function handleClearAll() {
        clearConfirmOpen = true;
    }

    function clearConfirmTitle(): string {
        return favoritesOnly ? '清空收藏' : '清空历史';
    }

    function clearConfirmHint(): string {
        return favoritesOnly
            ? '将删除全部收藏项目'
            : '将删除全部历史记录';
    }

    function clearConfirmAction(): string {
        return favoritesOnly ? '清空' : '清空';
    }

    async function confirmClearAll() {
        try {
            const command = favoritesOnly ? 'clear_favorite_items' : 'clear_history_only';
            await invoke(command);
            records = [];
        } catch (error) {
            console.error('Failed to clear history:', error);
        } finally {
            clearConfirmOpen = false;
            focusSearchInput(0);
        }
    }

    async function toggleFavoritesView() {
        favoritesOnly = !favoritesOnly;
        searchKeyword = '';
        await loadHistory(false);
        focusSearchInput(0);
    }

    function openExportFromSettings() {
        settingsOpen = false;
        focusSearchInput(0);
        void (async () => {
            try {
                await invoke('suspend_auto_hide', { ms: 10000 });
                const selected = await openDialog({
                    multiple: false,
                    directory: true
                });
                if (!selected || Array.isArray(selected)) return;

                const result = await invoke<ExportFavoritesResult>('export_favorites_to_path', {
                    path: selected,
                });
                window.alert(`导出完成，共 ${result.count} 条收藏\n文件: ${result.path}`);
            } catch (error) {
                window.alert(`导出失败: ${String(error)}`);
            }
        })();
    }

    function openImportFromSettings() {
        settingsOpen = false;
        focusSearchInput(0);
        void (async () => {
            try {
                await invoke('suspend_auto_hide', { ms: 10000 });
                const selected = await openDialog({
                    multiple: false,
                    directory: false,
                    filters: [{ name: 'JSON', extensions: ['json'] }]
                });
                if (!selected || Array.isArray(selected)) return;
                const count = await invoke<number>('import_favorites_from_path', { path: selected });
                window.alert(`导入完成，新增 ${count} 条收藏`);
                await loadHistory();
            } catch (error) {
                window.alert(`导入失败: ${String(error)}`);
            }
        })();
    }

    onMount(() => {
        void invoke('set_frontend_ready')
            .catch((error) => {
                console.error('Failed to notify frontend ready:', error);
            });

        // 监听系统主题变化
        const mediaQuery = window.matchMedia('(prefers-color-scheme: dark)');
        const handleThemeChange = () => {
            // 只有当前是系统主题模式时才更新
            const savedTheme = window.localStorage.getItem('clipper-theme');
            if (savedTheme === 'system' || savedTheme === 'auto') {
                applyTheme('system');
            }
        };
        mediaQuery.addEventListener('change', handleThemeChange);

        // 监听由后端启动，这里仅负责刷新 UI
        refreshInterval = setInterval(refreshHistory, 900);
        loadSettings();
        getVersion()
            .then((v) => {
                appVersion = v;
            })
            .catch(() => {});
        listen('open-settings', async () => {
            // 不重新加载设置，使用内存中已有的最新值
            settingsOpen = true;
        }).then((unlisten) => {
            unlistenOpenSettings = unlisten;
        }).catch((error) => {
            console.error('Failed to listen open-settings:', error);
        });
        listen('open-about', () => {
            settingsOpen = false;
            aboutOpen = true;
        }).then((unlisten) => {
            unlistenOpenAbout = unlisten;
        }).catch((error) => {
            console.error('Failed to listen open-about:', error);
        });
        listen('main-window-opened', async () => {
            const listEl = document.querySelector('.clipboard-list');
            if (listEl) {
                listEl.scrollTop = 0;
            }
            void resetSearchStateOnShow();
            focusSearchInput(16);
        }).then((unlisten) => {
            unlistenMainWindowOpened = unlisten;
        }).catch((error) => {
            console.error('Failed to listen main-window-opened:', error);
        });

        void refreshHistory();
        focusSearchInput(16);

        return () => {
            if (unlistenOpenSettings) {
                unlistenOpenSettings();
            }
            if (unlistenOpenAbout) {
                unlistenOpenAbout();
            }
            if (unlistenMainWindowOpened) {
                unlistenMainWindowOpened();
            }
            if (refreshInterval) {
                clearInterval(refreshInterval);
            }
        };
    });
</script>

<main class="app">
    <header class="header">
        <h1>{pageTitle()}</h1>
        <div class="header-actions">
            <button
                class="refresh-btn danger"
                onclick={handleClearAll}
                aria-label={favoritesOnly ? '清空收藏' : '清空历史'}
            >
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                    <path d="M3 6h18M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/>
                    <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6"/>
                    <path d="M10 11v6M14 11v6"/>
                </svg>
            </button>
            <button
                class="refresh-btn add-favorite-btn"
                onclick={openAddFavoriteDialog}
                aria-label="添加收藏"
            >
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                    <path d="M12 5v14M5 12h14"/>
                </svg>
            </button>
            <button
                class="refresh-btn favorite-toggle"
                class:active={favoritesOnly}
                onclick={toggleFavoritesView}
                aria-label={favoritesOnly ? '切换到记录' : '切换到收藏'}
            >
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                    <path d="M12 3l2.9 5.88 6.49.95-4.7 4.58 1.11 6.47L12 17.8l-5.8 3.08 1.1-6.47-4.7-4.58 6.5-.95z"/>
                </svg>
            </button>
        </div>
    </header>

    <div class="search-container">
        <SearchBar
            bind:this={searchBarRef}
            bind:value={searchKeyword}
            placeholder={`${records.length} 条记录`}
            onchange={handleSearch}
        />
    </div>

    <div class="list-container">
        <ClipboardList
            {records}
            {loading}
            oncopy={handleCopy}
            ondelete={handleDelete}
            onfavorite={handleFavorite}
            onpin={handlePinned}
            emptyTitle={emptyTitle()}
            emptyHint={emptyHint()}
        />
    </div>

    {#if addFavoriteOpen}
        <div class="confirm-backdrop">
            <div class="confirm-modal" role="dialog" aria-modal="true" aria-label="添加收藏">
                <h3>添加收藏</h3>
                <textarea
                    class="favorite-input"
                    bind:value={favoriteInput}
                    rows="4"
                    placeholder="输入内容..."
                ></textarea>
                <div class="confirm-actions">
                    <button class="cancel-btn" onclick={closeAddFavoriteDialog} disabled={addFavoriteSaving}>取消</button>
                    <button class="primary-btn" onclick={submitAddFavorite} disabled={addFavoriteSaving || !favoriteInput.trim()}>
                        {addFavoriteSaving ? '保存中...' : '添加'}
                    </button>
                </div>
            </div>
        </div>
    {/if}

    <SettingsModal
        open={settingsOpen}
        {settings}
        onsave={saveSettings}
        onopenimport={openImportFromSettings}
        onopenexport={openExportFromSettings}
        onclose={() => {
            settingsOpen = false;
            focusSearchInput(0);
        }}
    />

    {#if aboutOpen}
        <div class="confirm-backdrop">
            <div class="confirm-modal" role="dialog" aria-modal="true" aria-label="关于">
                <h3>关于 Clipper</h3>
                <p>版本：v{appVersion}</p>
                <p>作者：Jiaxin</p>
                <div class="confirm-actions">
                    <button class="primary-btn" onclick={() => (aboutOpen = false)}>知道了</button>
                </div>
            </div>
        </div>
    {/if}

    {#if clearConfirmOpen}
        <div class="confirm-backdrop">
            <div class="confirm-modal" role="alertdialog" aria-modal="true" aria-label="确认清空">
                <h3>{clearConfirmTitle()}</h3>
                <p>{clearConfirmHint()}</p>
                <div class="confirm-actions">
                    <button
                        class="cancel-btn"
                        onclick={() => {
                            clearConfirmOpen = false;
                            focusSearchInput(0);
                        }}
                    >
                        取消
                    </button>
                    <button class="danger-btn" onclick={confirmClearAll}>{clearConfirmAction()}</button>
                </div>
            </div>
        </div>
    {/if}
</main>

<style>
    :global(body) {
        margin: 0;
        padding: 0;
        overflow: hidden;
        background: transparent;
    }

    :global(*) {
        box-sizing: border-box;
    }

    :global(:root) {
        --bg-primary: rgba(255, 255, 255, 0.60);
        --bg-secondary: rgba(248, 249, 250, 0.8);
        --bg-hover: rgba(243, 244, 246, 0.75);
        --text-primary: #000000;
        --text-secondary: #ffffff;
        --text-tertiary: #6b7280;
        --border-color: rgba(209, 213, 219, 0.7);
        --accent-color: #2563eb;
        --accent-light: rgba(37, 99, 235, 0.12);
        --danger-color: #ef4444;
        --danger-light: rgba(239, 68, 68, 0.12);
        --scrollbar-track: rgba(238, 242, 247, 0.5);
        --scrollbar-thumb: rgba(198, 205, 216, 0.7);
        --scrollbar-thumb-hover: #aeb7c4;
        --glass-border: rgba(255, 255, 255, 0.8);
        --glass-shadow: rgba(0, 0, 0, 0.15);
    }

    :global([data-theme="dark"]) {
        --bg-primary: rgba(30, 30, 40, 0.65);
        --bg-secondary: rgba(40, 40, 55, 0.7);
        --bg-hover: rgba(82, 82, 95, 0.6);
        --text-primary: #f9fafb;
        --text-secondary: #9ca3af;
        --text-tertiary: #a1a7b0;
        --border-color: rgba(55, 65, 81, 0.5);
        --accent-color: #60a5fa;
        --accent-light: rgba(96, 165, 250, 0.2);
        --danger-color: #f87171;
        --danger-light: rgba(248, 113, 113, 0.2);
        --scrollbar-track: rgba(40, 40, 55, 0.3);
        --scrollbar-thumb: rgba(89, 98, 117, 0.5);
        --scrollbar-thumb-hover: #727e95;
        --glass-border: rgba(255, 255, 255, 0.1);
        --glass-shadow: rgba(0, 0, 0, 0.3);
    }

    /* 跟随系统主题 */
    @media (prefers-color-scheme: dark) {
        :global([data-theme="auto"]) {
            --bg-primary: rgba(30, 30, 40, 0.75);
            --bg-secondary: rgba(40, 40, 55, 0.7);
            --bg-hover: rgba(55, 55, 75, 0.6);
            --text-primary: #f9fafb;
            --text-secondary: #9ca3af;
            --text-tertiary: #a1a7b0;
            --border-color: rgba(55, 65, 81, 0.5);
            --accent-color: #60a5fa;
            --accent-light: rgba(96, 165, 250, 0.2);
            --danger-color: #f87171;
            --danger-light: rgba(248, 113, 113, 0.2);
            --scrollbar-track: rgba(40, 40, 55, 0.3);
            --scrollbar-thumb: rgba(89, 98, 117, 0.5);
            --scrollbar-thumb-hover: #727e95;
            --glass-border: rgba(255, 255, 255, 0.1);
            --glass-shadow: rgba(0, 0, 0, 0.3);
        }
    }

    :global(*) {
        scrollbar-width: thin;
        scrollbar-color: var(--scrollbar-thumb) var(--scrollbar-track);
    }

    :global(*::-webkit-scrollbar) {
        width: 10px;
        height: 10px;
    }

    :global(*::-webkit-scrollbar-track) {
        background: var(--scrollbar-track);
    }

    :global(*::-webkit-scrollbar-thumb) {
        background: var(--scrollbar-thumb);
        border-radius: 8px;
        border: 2px solid var(--scrollbar-track);
    }

    :global(*::-webkit-scrollbar-thumb:hover) {
        background: var(--scrollbar-thumb-hover);
    }

    .app {
        display: flex;
        flex-direction: column;
        height: 100vh;
        background: var(--bg-primary);
        font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif;
        border: 1px solid var(--border-color);
        box-shadow: 0 8px 32px var(--glass-shadow);
        overflow: hidden;
    }

    .header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        padding: 14px 18px;
        border-bottom: 1px solid var(--border-color);
        background: var(--bg-primary);
    }

    h1 {
        margin: 0;
        font-size: 15px;
        font-weight: 600;
        color: var(--text-primary);
        letter-spacing: -0.01em;
    }

    .header-actions {
        display: flex;
        gap: 8px;
    }

    .refresh-btn {
        display: flex;
        align-items: center;
        justify-content: center;
        width: 34px;
        height: 34px;
        padding: 0;
        border: 1px solid transparent;
        background: var(--bg-secondary);
        cursor: pointer;
        border-radius: 10px;
        transition: all 0.2s ease;
    }

    .refresh-btn:hover {
        background: var(--bg-hover);
        border-color: var(--border-color);
        transform: translateY(-1px);
    }

    .add-favorite-btn:hover {
        background: rgba(37, 99, 235, 0.14);
    }

    .add-favorite-btn:hover svg {
        color: var(--accent-color);
    }

    .refresh-btn.danger:hover {
        background: var(--danger-light);
    }

    .refresh-btn.danger:hover svg {
        color: var(--danger-color) !important;
    }

    .refresh-btn svg {
        width: 18px;
        height: 18px;
        color: var(--text-tertiary);
    }

    .search-container {
        padding: 14px 18px;
        border-bottom: 1px solid var(--border-color);
        background: var(--bg-secondary);
    }

    .list-container {
        flex: 1;
        min-height: 0;
        display: flex;
    }

    .favorite-toggle.active {
        background: rgba(245, 158, 11, 0.14);
    }

    .favorite-toggle:hover {
        background: rgba(245, 158, 11, 0.14);
    }

    .favorite-toggle:hover svg {
        color: #f59e0b;
        fill: rgba(245, 158, 11, 0.22);
    }

    .favorite-toggle.active svg {
        color: #f59e0b;
        fill: rgba(245, 158, 11, 0.22);
    }

    .favorite-input {
        width: 100%;
        min-height: 92px;
        resize: vertical;
        margin-top: 10px;
        border: 1px solid var(--border-color);
        border-radius: 8px;
        padding: 8px 10px;
        font-size: 13px;
        color: var(--text-primary);
        background: var(--bg-primary);
        outline: none;
    }

    .favorite-input:focus {
        border-color: var(--accent-color);
    }

    .confirm-backdrop {
        position: fixed;
        inset: 0;
        display: flex;
        align-items: center;
        justify-content: center;
        padding: 16px;
        background: rgba(0, 0, 0, 0.3);
        z-index: 50;
    }

    .confirm-modal {
        width: min(92vw, 360px);
        max-width: 100%;
        background: var(--bg-primary);
        border: 1px solid var(--glass-border);
        border-radius: 14px;
        padding: 18px;
        box-shadow: 0 16px 48px var(--glass-shadow);
    }

    .confirm-modal h3 {
        margin: 0 0 8px 0;
        font-size: 15px;
        color: var(--text-primary);
    }

    .confirm-modal p {
        margin: 0;
        font-size: 13px;
        color: var(--text-tertiary);
        line-height: 1.4;
    }

    .confirm-actions {
        margin-top: 14px;
        display: flex;
        justify-content: flex-end;
        gap: 8px;
    }

    .cancel-btn,
    .danger-btn,
    .primary-btn {
        height: 34px;
        padding: 0 14px;
        border-radius: 8px;
        border: 1px solid var(--border-color);
        background: var(--bg-secondary);
        color: var(--text-primary);
        cursor: pointer;
        font-size: 13px;
        font-weight: 500;
        transition: all 0.2s ease;
    }

    .danger-btn {
        border-color: var(--danger-color);
        background: var(--danger-color);
        color: #fff;
    }

    .primary-btn {
        border-color: var(--accent-color);
        background: var(--accent-color);
        color: #fff;
    }

    .cancel-btn:hover,
    .danger-btn:hover {
        transform: translateY(-1px);
        background: var(--bg-hover);
    }

    .primary-btn:hover {
        transform: translateY(-1px);
        box-shadow: 0 6px 14px rgba(0, 0, 0, 0.12);
    }

    .refresh-btn:active,
    .cancel-btn:active,
    .danger-btn:active,
    .primary-btn:active {
        transform: scale(0.96);
        box-shadow: none;
    }
</style>
