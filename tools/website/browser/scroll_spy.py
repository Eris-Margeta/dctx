"""Browser regression checks: serve website/, then run this file with the site's base URL.

Uses the same optional Playwright installation as e2e.py. Fails on the first regression.
"""
import sys

from playwright.sync_api import sync_playwright


BASE = sys.argv[1].rstrip("/")
CURRENT = 'a[aria-current="location"]'


def check_sections(page, path, nav):
    page.goto(BASE + path)
    if nav == ".toc-m":
        page.locator(".toc-m summary").click()
    links = page.locator(nav + ' a[href^="#"]').evaluate_all("els => els.map(e => e.hash)")
    assert len(links) > 3, path
    for fragment in links:
        # Scroll without changing the hash: the highlight must follow reading, including backwards.
        at_bottom = page.evaluate("""hash => {
            const target = document.getElementById(decodeURIComponent(hash.slice(1)));
            const line = parseFloat(getComputedStyle(document.documentElement).scrollPaddingTop);
            window.scrollTo(0, window.scrollY + target.getBoundingClientRect().top - line);
            return Math.ceil(window.scrollY + innerHeight) >= document.documentElement.scrollHeight - 1;
        }""", fragment)
        expected = links[-1] if at_bottom else fragment
        page.wait_for_function("""([nav, hash]) => {
            const links = document.querySelectorAll(nav + ' a[aria-current="location"]');
            return links.length === 1 && links[0].hash === hash;
        }""", arg=[nav, expected])
        if nav == ".toc":
            assert page.locator('.sidebar a[aria-current="page"]').get_attribute("href") == path
            assert page.locator(nav + " " + CURRENT).evaluate("""link => {
                const box = link.closest('.toc').getBoundingClientRect();
                const rect = link.getBoundingClientRect();
                return rect.top >= box.top - 1 && rect.bottom <= box.bottom + 1;
            }"""), (path, fragment, "active sidebar link is clipped")
    page.evaluate("window.scrollTo(0, 0)")
    page.wait_for_function("""([nav, hash]) =>
        document.querySelector(nav + ' a[aria-current="location"]').hash === hash
    """, arg=[nav, links[0]])
    if nav != ".toc-m":
        middle = links[len(links) // 2]
        page.goto(BASE + path + middle)
        page.wait_for_function("""([nav, hash]) => document.querySelector(nav + ' a[aria-current="location"]').hash === hash""", arg=[nav, middle])
        page.evaluate("hash => { location.hash = hash; }", links[0])
        page.wait_for_function("""([nav, hash]) => document.querySelector(nav + ' a[aria-current="location"]').hash === hash""", arg=[nav, links[0]])
        page.reload()
        page.wait_for_function("""([nav, hash]) => document.querySelector(nav + ' a[aria-current="location"]').hash === hash""", arg=[nav, links[0]])
    print("PASS sections", path, nav, len(links), flush=True)


with sync_playwright() as p:
    browser = p.chromium.launch()
    desktop = browser.new_context(viewport={"width": 1440, "height": 900}, reduced_motion="reduce")
    page = desktop.new_page()
    errors = []
    page.on("pageerror", lambda error: errors.append(str(error)))
    for path in ("/docs/readme/", "/docs/map-mode/", "/docs/spec/"):
        check_sections(page, path, ".toc")
    for path, nav in (("/cli/", ".toc-row"), ("/examples/", ".toc-row"),
                      ("/changelog/", ".toc-row"), ("/how-it-works/", ".how-list")):
        check_sections(page, path, nav)
    mobile = browser.new_context(viewport={"width": 390, "height": 844}, reduced_motion="reduce")
    check_sections(mobile.new_page(), "/docs/map-mode/", ".toc-m")

    # The shared version check runs in every bundle, including error and offline pages.
    desktop.route("**/build.json", lambda route: route.fulfill(json={"build_id": "0" * 64}))
    for path in ("/", "/docs/readme/", "/examples/", "/playground/", "/search/",
                 "/how-it-works/", "/404.html", "/playground/offline/"):
        page.goto(BASE + path)
        page.locator(".update").wait_for()
        assert page.locator(".update").get_attribute("role") == "status"
        with page.expect_navigation():
            page.get_by_role("button", name="Reload", exact=True).click()
        page.locator(".update").wait_for()
        page.get_by_role("button", name="Dismiss", exact=True).click()
        page.evaluate("document.dispatchEvent(new Event('visibilitychange'))")
        page.wait_for_timeout(100)
        assert page.locator(".update").count() == 0
        print("PASS version offer, reload and dismiss", path, flush=True)
    assert not errors, errors
    browser.close()
