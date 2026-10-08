(() => {
  try {
    if (localStorage.getItem('talos-theme') === 'light') {
      document.documentElement.setAttribute('data-theme', 'light')
    }
  } catch {
    // Theme bootstrap must never prevent the application shell from loading.
  }
})()
