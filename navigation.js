document.querySelectorAll('.lang-switch').forEach(switcher => {
  const trigger = switcher.querySelector('summary');
  const links = [...switcher.querySelectorAll('a')];
  switcher.addEventListener('keydown', event => {
    if (event.key === 'Escape' && switcher.open) {
      event.preventDefault();
      switcher.open = false;
      trigger.focus();
    }
    if (event.target === trigger && (event.key === 'ArrowDown' || event.key === 'ArrowUp')) {
      event.preventDefault();
      switcher.open = true;
      links[event.key === 'ArrowDown' ? 0 : links.length - 1].focus();
    }
  });
  document.addEventListener('click', event => {
    if (!switcher.contains(event.target)) switcher.open = false;
  });
  switcher.addEventListener('focusout', event => {
    if (!switcher.contains(event.relatedTarget)) switcher.open = false;
  });
});

// Google Analytics counts visits in consent mode with storage denied, so it
// writes no cookies. Its 177KB script waits for the first scroll, pointer or
// key, or 4s, so it never competes with the page itself.
window.dataLayer = window.dataLayer || [];
window.gtag = function () { window.dataLayer.push(arguments); };
gtag('consent', 'default', { analytics_storage: 'denied', ad_storage: 'denied', ad_user_data: 'denied', ad_personalization: 'denied' });
(function () {
  var events = ['scroll', 'pointerdown', 'pointermove', 'keydown', 'touchstart'];
  var timer;
  function load() {
    events.forEach(function (name) { window.removeEventListener(name, load); });
    clearTimeout(timer);
    gtag('js', new Date());
    gtag('config', 'G-V8NF2P2W71');
    var analytics = document.createElement('script');
    analytics.async = true;
    analytics.src = 'https://www.googletagmanager.com/gtag/js?id=G-V8NF2P2W71';
    document.head.appendChild(analytics);
  }
  function arm() {
    events.forEach(function (name) { window.addEventListener(name, load, { once: true, passive: true }); });
    timer = setTimeout(load, 4000);
  }
  // A hover-prerendered page the reader never opens must not count as a visit.
  if (document.prerendering) document.addEventListener('prerenderingchange', arm, { once: true });
  else arm();
})();

// Prerender an internal page while the pointer rests on its link, so the
// click swaps it in at once. Feeds, Markdown and other machine files stay out.
(function () {
  var skip = '[target=_blank], [download], [href^="/content/"], [href^="/feeds/"], [href$=".xml"], [href$=".md"], [href$=".txt"], [href$=".json"]';
  if (HTMLScriptElement.supports && HTMLScriptElement.supports('speculationrules')) {
    var rules = document.createElement('script');
    rules.type = 'speculationrules';
    rules.textContent = JSON.stringify({
      prerender: [{
        where: { and: [{ href_matches: '/*' }, { not: { selector_matches: skip } }] },
        eagerness: 'moderate'
      }]
    });
    document.head.appendChild(rules);
    return;
  }
  // Safari and Firefox: fetch the hovered page into the HTTP cache instead.
  var fetched = new Set([location.pathname]);
  var pending;
  document.addEventListener('mouseover', function (event) {
    var link = event.target.closest && event.target.closest('a[href^="/"]');
    if (!link || link.matches(skip) || fetched.has(link.pathname)) return;
    clearTimeout(pending);
    pending = setTimeout(function () {
      fetched.add(link.pathname);
      var hint = document.createElement('link');
      hint.rel = 'prefetch';
      hint.href = link.pathname;
      document.head.appendChild(hint);
    }, 100);
  });
  document.addEventListener('mouseout', function () { clearTimeout(pending); });
})();

(function () {
  // The contact counts are written in at build time and refreshed from
  // mole.fit's cached endpoint once the cards near the viewport; when it
  // is unreachable the written numbers stay.
  var stats = document.querySelectorAll('[data-community-stat]');
  if (!stats.length || typeof fetch !== 'function') return;

  function compact(value, sample) {
    var n = Number(value);
    if (value === null || value === undefined || value === '' || !Number.isSafeInteger(n) || n < 0) return null;
    if (n < 1000) return String(n);
    var s = (n / 1000).toFixed(1).replace(/\.0$/, '');
    if (/^(de|es|fr)(-|$)/i.test(document.documentElement.lang || '')) s = s.replace('.', ',');
    return s + (/k/.test(sample) ? 'k' : 'K');
  }

  function update(kind, value) {
    Array.prototype.forEach.call(document.querySelectorAll('[data-community-stat="' + kind + '"]'), function (el) {
      var match = (el.textContent || '').match(/\d+(?:[.,]\d+)?\s*[kK]?/);
      var next = match && compact(value, match[0]);
      if (next) el.textContent = el.textContent.replace(match[0], next);
    });
  }

  function refresh() {
    fetch('https://mole.fit/api/community-stats', {
      credentials: 'omit',
      headers: { Accept: 'application/json' }
    }).then(function (response) {
      if (!response.ok) throw new Error('community stats unavailable');
      return response.json();
    }).then(function (data) {
      update('x-followers', data.xFollowers);
      update('github-profile-stars', data.githubProfileStars);
      update('github-profile-forks', data.githubProfileForks);
      update('github-profile-followers', data.githubProfileFollowers);
    }).catch(function () {});
  }

  if (typeof IntersectionObserver !== 'function') return refresh();
  var observer = new IntersectionObserver(function (entries) {
    if (!entries.some(function (entry) { return entry.isIntersecting; })) return;
    observer.disconnect();
    refresh();
  }, { rootMargin: '700px 0px' });
  observer.observe(stats[0]);
})();
