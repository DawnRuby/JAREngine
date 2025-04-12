#![allow(
     dead_code,
     unused_variables,
     clippy::too_many_arguments,
     clippy::unnecessary_wraps
)]

use std::collections::HashSet;
use std::ffi::CStr;
use anyhow::{anyhow, Result};
use log::{debug, error, info, trace, warn};
use thiserror::Error;
use vulkanalia::window as vk_window;
use vulkanalia::prelude::v1_0::*;
use vulkanalia::Instance;
use vulkanalia::loader::{LibloadingLoader, LIBRARY};
use winit::dpi::LogicalSize;
use winit::event::{Event, WindowEvent};
use winit::event_loop::EventLoop;
use winit::raw_window_handle::HasRawWindowHandle;
use winit::window::{{Window, WindowBuilder}};

//Adding this for Apple support x.x
use vulkanalia::Version;
use vulkanalia::vk::ExtDebugUtilsExtension;

const PORTABILITY_MACOS_VERSION: Version = Version { major: 1, minor: 3, patch: 216 };


const VALIDATION_ENABLED: bool = cfg!(debug_assertions);
const VALIDATION_LAYER: vk::ExtensionName = vk::ExtensionName::from_bytes(b"VK_LAYER_KHRONOS_validation");

fn main() -> Result<()> {
     pretty_env_logger::init();

     // Create a Window to draw our stuff into
     let event_loop = EventLoop::new()?;
     let window = WindowBuilder::new()
         .with_title("")
         .with_inner_size(LogicalSize::new(800, 600))
         .build(&event_loop)?;
     let mut app = unsafe { App::create(&window)? };

     event_loop.run(move |event, elwt| {
          match event {
               Event::AboutToWait => window.request_redraw(),
               Event::WindowEvent { event, ..} => match event{
                    //Redraw Request Event Handling
                    WindowEvent::RedrawRequested =>
                         if !elwt.exiting() {
                              unsafe { app.render(&window) }.unwrap()
                         }

                    //Close Request Event Handling
                    WindowEvent::CloseRequested => {
                        elwt.exit();
                        unsafe { app.destroy(); }
                    }

                    //Handle all other events that we haven't handled ourselves by just discarding them for now
                    _ => {}
               }
               _ => {}
          }
     })?;

     Ok(())
}



///Make this more customizable via JSON configs or via external code :)
unsafe fn create_instance(
     window: &Window,
     entry: &Entry,
     data: &mut AppData
) -> Result<Instance> {
     let application_info = vk::ApplicationInfo::builder()
         .application_name(b"My Game\0")
         .application_version(vk::make_version(1, 1, 0))
         .engine_name(b"RAGEngine\0")
         .engine_version(vk::make_version(1, 1, 0))
         .api_version(vk::make_version(1, 4, 309));

     let available_layers = entry
         .enumerate_instance_layer_properties()?
         .iter()
         .map(|l| l.layer_name)
         .collect::<HashSet<_>>();

     if VALIDATION_ENABLED && !available_layers.contains(&VALIDATION_LAYER) {
          return Err(anyhow!("Validation layers are not supported."));
     }

     let layers =
         if VALIDATION_ENABLED { vec![VALIDATION_LAYER.as_ptr()] }
         else{ Vec::new() };

     let mut extensions = vk_window::get_required_instance_extensions(window)
         .iter()
         .map(|e| e.as_ptr())
         .collect::<Vec<_>>();

     let flags =
         if cfg!(target_os = "macos") && entry.version()? >= PORTABILITY_MACOS_VERSION
         {
              info!("Enabling macOS portability extensions");
              extensions.push(vk::KHR_GET_PHYSICAL_DEVICE_PROPERTIES2_EXTENSION.name.as_ptr());
              extensions.push(vk::KHR_PORTABILITY_ENUMERATION_EXTENSION.name.as_ptr());
              vk::InstanceCreateFlags::ENUMERATE_PORTABILITY_KHR
         }
         else{
              vk::InstanceCreateFlags::empty()
         };

     if VALIDATION_ENABLED {
          extensions.push(vk::EXT_DEBUG_UTILS_EXTENSION.name.as_ptr());
     }

     let mut info = vk::InstanceCreateInfo::builder()
         .application_info(&application_info)
         .enabled_layer_names(&layers)
         .enabled_extension_names(&extensions)
         .flags(flags);

     let extensions = vk_window::get_required_instance_extensions(window)
         .iter()
         .map(|e| e.as_ptr())
         .collect::<Vec<_>>();

     let mut debug_info = vk::DebugUtilsMessengerCreateInfoEXT::builder()
         .message_severity(vk::DebugUtilsMessageSeverityFlagsEXT::all())
         .message_type(
              vk::DebugUtilsMessageTypeFlagsEXT::GENERAL
                  | vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION
                  | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE,
         )
         .user_callback(Some(debug_callback));

     if VALIDATION_ENABLED {
          info = info.push_next(&mut debug_info);
     }

     let instance = entry.create_instance(&info, None)?;
     Ok(instance)
}

///Does the Debug Callback handling. Long term we want to push this to an API endpoint so we can
/// publish it locally via out application that handles all engine components.
extern "system" fn debug_callback(
     severity: vk::DebugUtilsMessageSeverityFlagsEXT,
     type_: vk::DebugUtilsMessageTypeFlagsEXT,
     data: *const vk::DebugUtilsMessengerCallbackDataEXT,
     //user_data: *mut std::os::raw::c_void,
     _: *mut std::ffi::c_void,) -> vk::Bool32 {
     let data = unsafe { *data };
     let message = unsafe { CStr::from_ptr(data.message) };

     if severity >= vk::DebugUtilsMessageSeverityFlagsEXT::ERROR {
          error!("{:?}", message);
     } else if severity >= vk::DebugUtilsMessageSeverityFlagsEXT::WARNING{
          warn!("{:?}", message);
     } else if severity >= vk::DebugUtilsMessageSeverityFlagsEXT::INFO{
          debug!("{:?}", message);
     }else{
          trace!("{:?}", message);
     }

     vk::FALSE
}



unsafe fn pick_physical_device(instance: &Instance, data: &mut AppData) -> Result<()> {
     for physical_device in instance.enumerate_physical_devices()? {
          let properties = instance.get_physical_device_properties(physical_device);

          if let Err(error) = check_physical_device(instance, data, physical_device){
               warn!("skipping physical device ({}) {:?}", properties.device_name, error);
          }else{
               info!("Selected physical device {}", properties.device_name);
               data.physical_device = physical_device;
               return Ok(());
          }
     }

     Err(anyhow!("no suitable physical device found"))
}

unsafe fn check_physical_device(
     instance: &Instance,
     data: &mut AppData,
     physical_device: vk::PhysicalDevice) -> Result<()> {
     let properties = instance.get_physical_device_properties(physical_device);

     if properties.device_type != vk::PhysicalDeviceType::DISCRETE_GPU {
          return Err(anyhow!(SuitabilityError("Only discrete GPUs are supported.")))
     }

     let features = instance.get_physical_device_features(physical_device);
     if features.geometry_shader != vk::TRUE{
          return Err(anyhow!(SuitabilityError("Missing GeometryShaders support.")))
     }
     let result = QueueFamilyIndices::get(instance, data, physical_device);
     if result.is_err(){
          return Err(anyhow!(SuitabilityError("Unable to find suitable QueueFamilyIndices that are required for support")));
     }

     Ok(())
}


unsafe fn create_logical_device(entry: &Entry, instance: &Instance, data: &mut AppData) -> Result<(Device)> {
     let indicies = QueueFamilyIndices::get(instance, data, data.physical_device)?;
     let queue_priorities = &[1.0];
     let queue_info = vk::DeviceQueueCreateInfo::builder()
         .queue_family_index(indicies.graphics)
         .queue_priorities(queue_priorities);

     let layers = if VALIDATION_ENABLED {
          vec![VALIDATION_LAYER.as_ptr()] }
     else{
          vec![]
     };

     let mut extensions = vec![];

     if (cfg!(target_os = "macos") && entry.version()? >= PORTABILITY_MACOS_VERSION){
          extensions.push(vk::KHR_PORTABILITY_SUBSET_EXTENSION.name.as_ptr());
     }

     let features = vk::PhysicalDeviceFeatures::builder();
     let queue_infos = &[queue_info];
     let info = vk::DeviceCreateInfo::builder()
         .queue_create_infos(queue_infos)
         .enabled_layer_names(&layers)
         .enabled_extension_names(&extensions)
         .enabled_features(&features);
     let device = instance.create_device(data.physical_device, &info, None)?;
     data.graphics_queue = device.get_device_queue(indicies.graphics, 0);
     Ok(device)
}

#[derive(Debug, Error)]
#[error("Missing {0}.")]
pub struct SuitabilityError(pub &'static str);

#[derive(Copy, Clone, Debug)]
pub struct QueueFamilyIndices {
     graphics: u32
}

/// Handle the Vulkan Associated Properties (Vulkan should do this for us ig)
#[derive(Clone, Debug, Default)]
struct AppData{
     messenger: vk::DebugUtilsMessengerEXT,
     physical_device: vk::PhysicalDevice,
     graphics_queue: vk::Queue,
}

///Literally our application (the Window and other things that we draw in it :)
#[derive(Clone, Debug)]
struct App {
     entry: Entry,
     instance: Instance,
     data: AppData,
     device: Device,
}

///Handles ensuring we support the right queue family indicies we can work with
impl QueueFamilyIndices {
     unsafe fn get(
          instance: &Instance,
          data: &AppData,
          physical_device: vk::PhysicalDevice) -> Result<Self> {
          let properties = instance.get_physical_device_queue_family_properties(physical_device);

          let graphics = properties.iter()
              .position(|p| p.queue_flags.contains(vk::QueueFlags::GRAPHICS))
              .map(|i| i as u32);

          if let Some(graphics) = graphics{
               Ok(Self{graphics})
          } else{
               Err(anyhow!(SuitabilityError("Missing some of our required queue families.")))
          }
     }
}

impl App {
     ///Creates our app
     unsafe fn create(window: &Window) -> Result<Self> {
          let loader = LibloadingLoader::new(LIBRARY)?;
          let entry = Entry::new(loader).map_err(|b| anyhow!("{}", b))?;
          let mut data = AppData::default();
          let instance = create_instance(window, &entry, &mut data)?;
          pick_physical_device(&instance, &mut data);
          let device = create_logical_device(&entry, &instance, &mut data)?;
          Ok(Self { entry, instance, data, device })
     }

     ///Renders a frame for our app
     unsafe fn render(&mut self, window: &Window) -> Result<()>{
          Ok(())
     }

     ///Kills our App
     unsafe fn destroy(&mut self) {
          self.device.destroy_device(None);
          if VALIDATION_ENABLED{
               self.instance.destroy_debug_utils_messenger_ext(self.data.messenger, None);
          }

          self.instance.destroy_instance(None);
     }
}