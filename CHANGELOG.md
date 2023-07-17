# [1.1.0](https://github.com/wasmcompute/tokactor/compare/v1.0.0...v1.1.0) (2023-07-17)


### Features

* accept tcp input from the `World` ([e87c2f5](https://github.com/wasmcompute/tokactor/commit/e87c2f5d94080516e2dd33e5b10a4b443e6c5c15))
* add debugging commands ([4b2d10d](https://github.com/wasmcompute/tokactor/commit/4b2d10d64b07cf57ffabd865e658e81e10c3470c))
* Add HTTP example ([f56f25b](https://github.com/wasmcompute/tokactor/commit/f56f25bfcc0c53799d28bcedec08b9485d11f531))
* add tracing subscriber to all examples ([84d41c6](https://github.com/wasmcompute/tokactor/commit/84d41c675cb9bb6e2ce29c21b8361f72fced729f))
* Added stateful world example; Updated tcp model ([7853f0c](https://github.com/wasmcompute/tokactor/commit/7853f0c0ff07a60bf5afcf2c4d3c6d27c805d049))
* Allow for ability to wait for actor to exit by itself ([f4f981e](https://github.com/wasmcompute/tokactor/commit/f4f981eb2e8b38501a7e3b3481f8fe96ceaab40c))
* **closes #1:** add tracing ([016099a](https://github.com/wasmcompute/tokactor/commit/016099a318c45936982a400f7c141130ce03da3f)), closes [#1](https://github.com/wasmcompute/tokactor/issues/1)
* remove the need to `impl Message` ([e752d89](https://github.com/wasmcompute/tokactor/commit/e752d89aa4eb02d9c31e8a64cba6b73550c53d54))
* Set a max amount of actors that can spawn at one time ([3a70bdc](https://github.com/wasmcompute/tokactor/commit/3a70bdcbb894f7c3927ab487264041adc38c5ac3))
* Upgrade version ([0136355](https://github.com/wasmcompute/tokactor/commit/01363558e37ad4bde37d90cf1b8cb87be03a40d5))

# 1.0.0 (2023-06-06)


### Features

* **#3:** Added more comments and added features to actor ref ([ed34387](https://github.com/wasmcompute/tokactor/commit/ed3438756bc92d7b1a9b0ff7fe4da9f7b0d682af)), closes [#3](https://github.com/wasmcompute/tokactor/issues/3)
* add Router actor ([6f47a40](https://github.com/wasmcompute/tokactor/commit/6f47a40e5ecedc94638a0f6fcf4897cac964fd57))
* added ability for asks to return async task ([9b94c86](https://github.com/wasmcompute/tokactor/commit/9b94c86754946ad6c3715b767cc13920f6f5eb2d))
* added ability to hide actor ([a8613bd](https://github.com/wasmcompute/tokactor/commit/a8613bd7515470d6bcd697bccbfd5621dbacd043))
* added ability to run actors with many generic types ([6f60a53](https://github.com/wasmcompute/tokactor/commit/6f60a531412a582f2ca33e75af4980d09891584b))
* added workflow using functions and actor refs ([2f981cd](https://github.com/wasmcompute/tokactor/commit/2f981cd6fff2cf7c6dafcaf0e0e5ab840121627b))
* allow for `ask` to implement `AsyncAsk` ([a7fdb11](https://github.com/wasmcompute/tokactor/commit/a7fdb11fff0044d201cd51c3081fac9fe5090370))
* allow unwrapping of `SendError<M>` ([aa1fb5c](https://github.com/wasmcompute/tokactor/commit/aa1fb5c3fcee88ae1d2756f2eaa66e97d7d23e0f))
* expose tokio::main and fix warnings ([3b15731](https://github.com/wasmcompute/tokactor/commit/3b1573151aa3a05421e5f1ebfc56aa520868cf7b))
* remove requirement for const str name on actor ([bf77b82](https://github.com/wasmcompute/tokactor/commit/bf77b8249160069932a1a721d156f8001b5950f0))
* removed actor execution logic to new file ([335a341](https://github.com/wasmcompute/tokactor/commit/335a34149b562222b4b1ac50046e8038231bec2f))
