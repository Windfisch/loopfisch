const path = require('path');
const CopyWebpackPlugin = require('copy-webpack-plugin');
const VueLoaderPlugin = require('vue-loader/lib/plugin')

 
module.exports = {
    //context: path.join(__dirname, 'your-app'),
    //context: __dirname,
	module: {
		rules: [
			{
				test: /\.vue$/,
				loader: 'vue-loader'
			},
  {
    test: /\.css$/,
    use: [
      'vue-style-loader',
      'css-loader'
    ]
  },
		]
	},
    plugins: [
        new CopyWebpackPlugin({
            patterns: [
                { from: 'static' }
            ]
        }),
		new VueLoaderPlugin()
    ],
    devServer: {
        contentBase: "./dist",
    },
    resolve: {
        alias: {
            'vue$': 'vue/dist/vue.esm.js' // 'vue/dist/vue.common.js' for webpack 1
        }
    }
};
