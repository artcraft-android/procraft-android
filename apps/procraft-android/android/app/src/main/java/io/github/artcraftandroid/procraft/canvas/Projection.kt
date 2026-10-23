package io.github.artcraftandroid.procraft.canvas

import android.app.Presentation
import android.content.Context
import android.graphics.Color
import android.graphics.drawable.ColorDrawable
import android.os.Bundle
import android.view.Display
import android.view.SurfaceHolder
import android.view.SurfaceView

/** Project canvas on [display]: one full-screen surface the session draws the canvas into. */
class ProjectionPresentation(context: Context, display: Display, private val vm: CanvasViewModel) : Presentation(context, display) {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window?.setBackgroundDrawable(ColorDrawable(Color.BLACK))
        val view = SurfaceView(context)
        view.holder.addCallback(object : SurfaceHolder.Callback {
            override fun surfaceCreated(holder: SurfaceHolder) = Unit

            override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {
                vm.setProjectionSurface(holder.surface, width, height)
            }

            override fun surfaceDestroyed(holder: SurfaceHolder) {
                vm.setProjectionSurface(null)
            }
        })
        setContentView(view)
    }
}
