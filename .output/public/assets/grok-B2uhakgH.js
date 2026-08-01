function e(e){let t=[];for(let n of e.split(`
`)){let e=n.match(/^\s*HOST_CMD:\s*(.+)\s*$/i);e?.[1]&&t.push(e[1].trim())}return t}export{e as extractHostCommands};