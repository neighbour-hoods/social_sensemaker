const path = require('path')
const tape = require('tape')

const { Orchestrator, Config, combine, tapeExecutor, localOnly } = require('@holochain/tryorama')


//helpers

const buildRunner = () => new Orchestrator({
  middleware: combine(
    tapeExecutor(tape),
    localOnly,
  ),
})

// DNA loader, to be used with `buildTestScenario` when constructing DNAs for testing
const getDNA = ((dnas) => (name) => (dnas[name]))({
  'scaffolding': path.resolve(__dirname, '../happs/scaffolding/scaffolding.dna'),
})




// main test script

const runner = buildRunner()
const config = Config.gen()

runner.registerScenario('Basic DSL program compilation', async (scenario, t) => {
  const [player] = await scenario.players([config])
  const [[firstHapp]] = await player.installAgentsHapps(
  [ // (don't quote me) agent key 1
    [ // agent app bundle 1
      [  // app bundle DNAs
        getDNA('scaffolding'),
      ]
    ]
  ])
//  const appCellIds = firstHapp.cells.map(c => c.cellNick.match(/(\w+)\.dna$/)[1])
  
  const scaffoldingApp = firstHapp.cells[0]

  const result = await scaffoldingApp.call('interpreter', 'test_output')
  console.log('did a call!', result)
})
