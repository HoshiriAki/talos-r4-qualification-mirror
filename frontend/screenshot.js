const puppeteer = require('puppeteer');
const fs = require('fs');
const path = require('path');

(async () => {
  const browser = await puppeteer.launch({ headless: 'new' });
  const page = await browser.newPage();
  await page.setViewport({ width: 1920, height: 1080 });
  
  try {
    console.log('Navigating to http://localhost:5173...');
    await page.goto('http://localhost:5173', { waitUntil: 'networkidle0', timeout: 30000 });
    
    const screenshotDir = '.harness/screenshots';
    if (!fs.existsSync(screenshotDir)) {
      fs.mkdirSync(screenshotDir, { recursive: true });
    }
    
    // Full page screenshot
    await page.screenshot({ 
      path: path.join(screenshotDir, 'dashboard-full.png'),
      fullPage: true 
    });
    console.log('? Screenshot saved: dashboard-full.png');
    
    // Sidebar screenshot
    const sidebar = await page.$('aside.sidebar');
    if (sidebar) {
      await sidebar.screenshot({ path: path.join(screenshotDir, 'sidebar.png') });
      console.log('? Screenshot saved: sidebar.png');
    }
    
    console.log('\nScreenshots saved in .harness/screenshots/');
  } catch (error) {
    console.error('Error:', error.message);
    process.exit(1);
  } finally {
    await browser.close();
  }
})();
